use criterion::{criterion_group, criterion_main, Criterion};
use fast_image_resize::images::{Image, ImageRef};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use rustview::thumbnails::{calculate_aspect_dimensions, ThumbnailFormat, ThumbnailGenerator};
use std::hint::black_box;

fn thumbnail_benchmark(c: &mut Criterion) {
    let src_w = 3840;
    let src_h = 2160;
    let src_data = vec![128u8; (src_w * src_h * 3) as usize];
    let (dst_w, dst_h) = calculate_aspect_dimensions(src_w, src_h, 256);

    // 1. SIMD Resize Algorithm Comparisons (4K -> 256x144)
    c.bench_function("simd_resize_4k_catmullrom", |b| {
        b.iter(|| {
            let src_image =
                ImageRef::new(src_w, src_h, black_box(&src_data), PixelType::U8x3).unwrap();
            let mut dst_image = Image::new(dst_w, dst_h, PixelType::U8x3);
            let mut resizer = Resizer::new();
            let options =
                ResizeOptions::default().resize_alg(ResizeAlg::Convolution(FilterType::CatmullRom));

            resizer
                .resize(&src_image, &mut dst_image, &options)
                .unwrap();
            black_box(dst_image.into_vec());
        });
    });

    c.bench_function("simd_resize_4k_bilinear", |b| {
        b.iter(|| {
            let src_image =
                ImageRef::new(src_w, src_h, black_box(&src_data), PixelType::U8x3).unwrap();
            let mut dst_image = Image::new(dst_w, dst_h, PixelType::U8x3);
            let mut resizer = Resizer::new();
            let options =
                ResizeOptions::default().resize_alg(ResizeAlg::Convolution(FilterType::Bilinear));

            resizer
                .resize(&src_image, &mut dst_image, &options)
                .unwrap();
            black_box(dst_image.into_vec());
        });
    });

    c.bench_function("simd_resize_4k_nearest", |b| {
        b.iter(|| {
            let src_image =
                ImageRef::new(src_w, src_h, black_box(&src_data), PixelType::U8x3).unwrap();
            let mut dst_image = Image::new(dst_w, dst_h, PixelType::U8x3);
            let mut resizer = Resizer::new();
            let options = ResizeOptions::default().resize_alg(ResizeAlg::Nearest);

            resizer
                .resize(&src_image, &mut dst_image, &options)
                .unwrap();
            black_box(dst_image.into_vec());
        });
    });

    // 2. WebP vs JPEG Compression & Decompression Benchmarks
    let generator = ThumbnailGenerator::new();
    let sample_rgb = vec![140u8; (dst_w * dst_h * 3) as usize];

    c.bench_function("encode_webp_quality_80", |b| {
        b.iter(|| {
            let encoded = generator
                .compress(
                    black_box(&sample_rgb),
                    dst_w,
                    dst_h,
                    ThumbnailFormat::Webp,
                    80,
                )
                .unwrap();
            black_box(encoded);
        });
    });

    c.bench_function("encode_webp_quality_40", |b| {
        b.iter(|| {
            let encoded = generator
                .compress(
                    black_box(&sample_rgb),
                    dst_w,
                    dst_h,
                    ThumbnailFormat::Webp,
                    40,
                )
                .unwrap();
            black_box(encoded);
        });
    });

    c.bench_function("encode_jpeg_quality_80", |b| {
        b.iter(|| {
            let encoded = generator
                .compress(
                    black_box(&sample_rgb),
                    dst_w,
                    dst_h,
                    ThumbnailFormat::Jpeg,
                    80,
                )
                .unwrap();
            black_box(encoded);
        });
    });

    let compressed_webp = generator
        .compress(&sample_rgb, dst_w, dst_h, ThumbnailFormat::Webp, 80)
        .unwrap();
    let compressed_jpeg = generator
        .compress(&sample_rgb, dst_w, dst_h, ThumbnailFormat::Jpeg, 80)
        .unwrap();

    c.bench_function("decompress_webp_slice", |b| {
        b.iter(|| {
            let raw = generator.decompress(black_box(&compressed_webp)).unwrap();
            black_box(raw);
        });
    });

    c.bench_function("decompress_jpeg_slice", |b| {
        b.iter(|| {
            let raw = generator.decompress(black_box(&compressed_jpeg)).unwrap();
            black_box(raw);
        });
    });
}

criterion_group!(benches, thumbnail_benchmark);
criterion_main!(benches);
