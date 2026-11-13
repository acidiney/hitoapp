use crate::{
    artwork_rendition_http::{Captured, render},
    artwork_rendition_images::Case,
};

#[tokio::test]
async fn a_large_jpeg_is_oriented_before_its_opaque_managed_rendition_is_encoded() {
    let captured = render(Case::OrientedJpeg).await;
    let image = image::load_from_memory(&captured.bytes).unwrap().to_rgb8();
    assert_eq!(image.dimensions(), (450, 600));
    assert_eq!(captured.content_type, "image/jpeg");
    let points: [((u32, u32), [u8; 3]); 4] = [
        ((112, 150), [30, 60, 220]),
        ((337, 150), [220, 40, 30]),
        ((112, 450), [220, 190, 30]),
        ((337, 450), [40, 200, 60]),
    ];
    let mut maximum_error: u8 = 0;
    for ((x, y), expected) in points {
        for (actual, expected) in image.get_pixel(x, y).0.into_iter().zip(expected) {
            maximum_error = maximum_error.max(actual.abs_diff(expected));
        }
    }
    assert!(
        maximum_error <= 20,
        "The oriented quadrants must retain their colors"
    );
    eprintln!(
        "artwork_rendition case=oriented_jpeg source_bytes={} retained_bytes={} width=450 height=600 maximum_sample_error={maximum_error}",
        captured.source_bytes,
        captured.bytes.len()
    );
}

#[tokio::test]
async fn a_large_transparent_png_keeps_its_transparency_and_edge_color() {
    transparent(render(Case::Transparent).await, "rgba8");
}

#[tokio::test]
async fn a_large_palette_png_keeps_its_transparency_and_edge_color() {
    transparent(render(Case::Palette).await, "palette");
}

#[tokio::test]
async fn a_large_sixteen_bit_png_keeps_its_transparency_and_edge_color() {
    transparent(render(Case::SixteenBit).await, "rgba16");
}

fn transparent(captured: Captured, case: &str) {
    let image = image::load_from_memory(&captured.bytes).unwrap().to_rgba8();
    assert_eq!(image.dimensions(), (600, 450));
    assert_eq!(captured.content_type, "image/png");
    assert_eq!(image.get_pixel(10, 10).0[3], 0);
    assert!(image.get_pixel(300, 225).0[3].abs_diff(128) <= 1);
    let mut maximum_error: u8 = 0;
    let mut edges: usize = 0;
    for x in 146..155 {
        let pixel = image.get_pixel(x, 225).0;
        if pixel[3] > 0 && pixel[3] < 128 {
            edges += 1;
            for (actual, expected) in pixel[..3].iter().copied().zip([25, 180, 70]) {
                maximum_error = maximum_error.max(actual.abs_diff(expected));
            }
        }
    }
    assert!(
        edges > 0,
        "The resized transparency boundary must remain visible"
    );
    for (actual, expected) in image.get_pixel(300, 225).0[..3]
        .iter()
        .copied()
        .zip([25, 180, 70])
    {
        maximum_error = maximum_error.max(actual.abs_diff(expected));
    }
    assert!(
        maximum_error <= 3,
        "Transparent hidden colors must not bleed into the green edge"
    );
    eprintln!(
        "artwork_rendition case={case} source_bytes={} retained_bytes={} width=600 height=450 maximum_sample_error={maximum_error} edge_samples={edges}",
        captured.source_bytes,
        captured.bytes.len()
    );
}
