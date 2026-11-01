use hito_plugin_sdk::{
    ArtistSearchCandidate, ArtistSearchPage, ArtistSearchRequest, CatalogArtistRef,
    MAX_ARTIST_SEARCH_PAGE_BYTES,
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering::Relaxed},
};

thread_local! {
    static OBSERVING: Cell<bool> = const { Cell::new(false) };
}

static LARGEST_REQUEST: AtomicUsize = AtomicUsize::new(0);

struct ObservedSystem;

#[global_allocator]
static ALLOCATOR: ObservedSystem = ObservedSystem;

fn observe(size: usize) {
    let _ = OBSERVING.try_with(|observing| {
        if observing.get() {
            LARGEST_REQUEST.fetch_max(size, Relaxed);
        }
    });
}

// SAFETY: Every pointer, layout and allocation operation is forwarded unchanged to System.
unsafe impl GlobalAlloc for ObservedSystem {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        observe(layout.size());
        // SAFETY: The caller provides the allocation layout required by System.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        observe(layout.size());
        // SAFETY: The caller provides the allocation layout required by System.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        observe(new_size);
        // SAFETY: The original System allocation and its supplied layout are preserved.
        unsafe { System.realloc(pointer, layout, new_size) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: The original System allocation is released with its supplied layout.
        unsafe { System.dealloc(pointer, layout) }
    }
}

struct AllocationWindow;

impl AllocationWindow {
    fn begin() -> Self {
        LARGEST_REQUEST.store(0, Relaxed);
        OBSERVING.with(|observing| {
            assert!(
                !observing.replace(true),
                "Allocation windows must not overlap"
            );
        });
        Self
    }

    fn finish(self) -> usize {
        OBSERVING.with(|observing| observing.set(false));
        LARGEST_REQUEST.load(Relaxed)
    }
}

impl Drop for AllocationWindow {
    fn drop(&mut self) {
        let _ = OBSERVING.try_with(|observing| observing.set(false));
    }
}

#[test]
fn oversized_artist_search_validation_bounds_its_largest_allocation_request() {
    let request = ArtistSearchRequest {
        query: "Owned resource fixture".into(),
        cursor: None,
        snapshot: None,
        limit: 20,
    };
    request.validate().expect("The owned request must be valid");
    let page = ArtistSearchPage {
        query: request.query.clone(),
        snapshot: None,
        items: (0..20)
            .map(|index| ArtistSearchCandidate {
                artist: CatalogArtistRef {
                    id: format!("artist:owned/{index}"),
                    name: "\"".repeat(2048),
                },
                kind: None,
                disambiguation: Some("\\".repeat(2048)),
                country: None,
                begin_year: None,
            })
            .collect(),
        total_count: Some(20),
        next_cursor: None,
    };
    for candidate in &page.items {
        candidate
            .validate()
            .expect("Every owned candidate must be field-valid");
    }
    let complete_page = serde_json::to_vec(&page).unwrap();
    assert!(complete_page.len() > MAX_ARTIST_SEARCH_PAGE_BYTES * 2);
    drop(complete_page);

    let calibration = AllocationWindow::begin();
    let calibration_bytes = black_box(vec![0_u8; black_box(MAX_ARTIST_SEARCH_PAGE_BYTES + 1)]);
    black_box(&calibration_bytes);
    let calibration_largest = calibration.finish();
    assert!(
        calibration_largest > MAX_ARTIST_SEARCH_PAGE_BYTES,
        "The observer must detect its owned oversized calibration allocation"
    );
    drop(calibration_bytes);

    let validation = AllocationWindow::begin();
    let result = black_box(&page).validate_for(black_box(&request));
    let largest_request = validation.finish();
    assert!(
        result.is_err(),
        "The field-valid oversized page must be rejected"
    );
    assert!(
        largest_request <= MAX_ARTIST_SEARCH_PAGE_BYTES,
        "Artist search validation requested an allocation of {largest_request} bytes"
    );
}
