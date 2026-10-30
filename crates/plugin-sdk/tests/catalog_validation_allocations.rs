use hito_plugin_sdk::{
    ArtistDiscographyPage, ArtistDiscographyRequest, CatalogArtistRef, CatalogCreditMember,
    CatalogReleaseGroup, CatalogReleaseGroupKind, MAX_CATALOG_PAGE_BYTES,
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
                "allocation windows must not overlap"
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
fn oversized_discography_validation_bounds_its_largest_allocation_request() {
    let request = ArtistDiscographyRequest {
        artist_id: "artist:owned/resource-fixture".into(),
        cursor: None,
        snapshot: None,
        limit: 50,
        release_type: None,
    };
    request.validate().expect("the owned request must be valid");
    let member = CatalogCreditMember {
        artist: Some(CatalogArtistRef {
            id: request.artist_id.clone(),
            name: "\"".repeat(2048),
        }),
        credited_name: "\"".repeat(2048),
        join_phrase: "\"".repeat(128),
    };
    let items = (0..50)
        .map(|index| CatalogReleaseGroup {
            id: format!("group:owned/{index}"),
            title: "Owned resource fixture".into(),
            artist_credit: Some(vec![member.clone(); 32]),
            kind: Some(CatalogReleaseGroupKind::Album),
            secondary_types: vec![],
            first_release_date: None,
            revision: None,
        })
        .collect();
    let page = ArtistDiscographyPage {
        artist_id: request.artist_id.clone(),
        snapshot: None,
        items,
        total_count: Some(50),
        next_cursor: None,
    };
    for group in &page.items {
        group
            .validate()
            .expect("each owned group must be field-valid");
    }
    let credited_name_bytes: usize = page
        .items
        .iter()
        .flat_map(|group| group.artist_credit.as_ref().expect("owned credits exist"))
        .map(|member| member.credited_name.len())
        .sum();
    assert!(credited_name_bytes > MAX_CATALOG_PAGE_BYTES);

    let calibration = AllocationWindow::begin();
    let calibration_bytes = black_box(vec![0_u8; black_box(MAX_CATALOG_PAGE_BYTES + 1)]);
    black_box(&calibration_bytes);
    let calibration_largest = calibration.finish();
    assert!(
        calibration_largest > MAX_CATALOG_PAGE_BYTES,
        "the allocation observer must detect its owned calibration request"
    );
    drop(calibration_bytes);

    let validation = AllocationWindow::begin();
    let result = black_box(&page).validate_for(black_box(&request));
    let largest_request = validation.finish();

    assert_eq!(
        result,
        Err("The provider discography page exceeds the size limit")
    );
    assert!(
        largest_request <= MAX_CATALOG_PAGE_BYTES,
        "discography validation requested an allocation of {largest_request} bytes"
    );
}
