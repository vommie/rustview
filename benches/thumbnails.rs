use criterion::{Criterion, criterion_group, criterion_main};
use fast_image_resize::images::{Image, ImageRef};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use std::hint::black_box;

fn thumbnail_benchmark(c: &mut Criterion) {
    // 1920x1080 synthetic RGB8 frame
    let src_w = 1920;
    let src_h = 1080;
    let src_data = vec![128u8; (src_w * src_h * 3) as usize];

    c.bench_function("simd_resize_1080p_to_256x256", |b| {
        b.iter(|| {
            let src_image =
                ImageRef::new(src_w, src_h, black_box(&src_data), PixelType::U8x3).unwrap();
            let mut dst_image = Image::new(256, 256, PixelType::U8x3);
            let mut resizer = Resizer::new();
            let options =
                ResizeOptions::default().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3));

            resizer
                .resize(&src_image, &mut dst_image, &options)
                .unwrap();
            black_box(dst_image.into_vec());
        });
    });
}

criterion_group!(benches, thumbnail_benchmark);
criterion_main!(benches);
