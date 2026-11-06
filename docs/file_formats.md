# File formats

Which files RustView takes, how it decodes them, how the user can switch formats off, and how that was verified.
Code: `src/media/format_table.rs` (the table), `format_policy.rs` (the switches), `detector.rs` (header signatures),
`scanner.rs`, `svg.rs`, `tile_grid.rs`, `orientation.rs`; Settings page: `src/settings/formats_page.rs`,
`ui/settings_page_formats.slint`, `ui/settings_format_row.slint`.

## One table

`FORMAT_SPECS` lists every format once: stable settings id, short and long name, extensions, kind (image or video), group,
decoder, MIME type, notes. The scanner, the tree walker, the detector, the clipboard/"Open With" MIME lookup and the
Settings page all read it. `MediaFormat` (the enum the library stores) **only grows at the end, after `Unknown`**: the
library keeps the variant index (bincode), so reordering or inserting would corrupt existing records
(`table_follows_enum_order` and `existing_variants_keep_their_stored_index` guard this). There is no migration: records of
formats added later use new indexes that an older RustView cannot read.

Adding a format: append the enum variant and the table row, add a header signature to `detector.rs` only if it is reliable,
generate a sample in the test environment (`Formats/`), check it end to end (scan, thumbnail, viewer), and extend this document.

### Images (28)

| Format | id | Extensions | Group | Decoder |
|---|---|---|---|---|
| AVIF | `avif` | .avif .avifs | Common | FFmpeg (libdav1d) |
| Windows Bitmap | `bmp` | .bmp .dib | Common | built-in |
| GIF | `gif` | .gif | Common | built-in |
| HEIC / HEIF | `heic` | .heic .heif .hif | Common | FFmpeg (HEVC) |
| Windows Icon | `ico` | .ico | Common | built-in |
| JPEG | `jpeg` | .jpg .jpeg .jpe .jfif .jif | Common | built-in |
| PNG | `png` | .png .apng | Common | built-in |
| SVG | `svg` | .svg | Common | built-in (resvg) |
| TIFF | `tiff` | .tif .tiff | Common | built-in |
| WebP | `webp` | .webp | Common | built-in |
| DirectDraw Surface | `dds` | .dds | Specialized | built-in |
| Digital Picture Exchange | `dpx` | .dpx | Specialized | FFmpeg |
| OpenEXR | `exr` | .exr | Specialized | built-in |
| Radiance HDR | `hdr` | .hdr | Specialized | built-in |
| JPEG 2000 | `jp2` | .jp2 .j2k .j2c .jpc .jpf .jpx | Specialized | FFmpeg (OpenJPEG) |
| JPEG XL | `jxl` | .jxl | Specialized | FFmpeg (libjxl) |
| Portable anymap | `pnm` | .pnm .ppm .pgm .pbm .pam .pfm | Specialized | built-in (PFM: FFmpeg) |
| Photoshop Document | `psd` | .psd | Specialized | FFmpeg (flattened composite) |
| Quite OK Image | `qoi` | .qoi | Specialized | built-in |
| Targa | `tga` | .tga | Specialized | built-in |
| Farbfeld | `farbfeld` | .ff | Legacy | built-in |
| PC Paintbrush | `pcx` | .pcx | Legacy | FFmpeg |
| Sun Raster | `sunraster` | .ras .sun | Legacy | FFmpeg |
| Silicon Graphics Image | `sgi` | .sgi .rgb .bw | Legacy | FFmpeg |
| Wireless Bitmap | `wbmp` | .wbmp | Legacy | FFmpeg |
| X BitMap / X PixMap / X Window Dump | `xbm` `xpm` `xwd` | .xbm .xpm .xwd | Legacy | FFmpeg |

"Built-in" is the `image` crate (features `jpeg png webp gif tiff bmp ico tga pnm qoi hdr exr dds ff`). Its `avif` feature
is only the AVIF *encoder*, so it is not enabled (AVIF decoding would need libdav1d bindings); AVIF goes through FFmpeg.
Every image falls back to FFmpeg when the `image` crate cannot decode it.

### Videos (22)

MP4 (.mp4 .m4v .f4v), QuickTime (.mov .qt), Matroska (.mkv .mk3d), WebM, AVI (.avi .divx), MPEG-1/2 (.mpg .mpeg .mpe .vob
.m1v .m2v), Flash Video, Windows Media (.wmv .asf), MPEG-TS (.ts .m2t), AVCHD (.m2ts .mts), Ogg Video (.ogv .ogm), 3GPP
(.3gp .3g2 .3gpp), MXF, DV (.dv .dif), RealMedia (.rm .rmvb), Windows TV (.wtv .dvr-ms), GXF, YUV4MPEG2, IVF, NUT, and the raw
streams H.264 (.h264 .264) and HEVC (.hevc .h265). Thumbnails and metadata come from FFmpeg (or mpv, Settings >
Performance), playback from mpv: anything FFmpeg demuxes plays.

Left out on purpose: audio-only containers (.mp3 .flac .m4a .ogg .wav ...), `.ogg` (audio or video, so only `.ogv`/`.ogm`),
`.mod` (tracker music), `.dat`, `.str`, `.bin`, subtitles, playlists, raw frame dumps (.yuv .rgb), Flash `.swf` (mostly
interactive animations without a video stream: they would all show up as damaged), game cutscene formats (.bik .smk .roq
.vqa ...), camcorder `.mod/.tod/.vro`, surveillance `.dav`. Any of them is one table row away.

## Image or video, extension or content

Two decisions are kept apart:

* **The extension decides which switch a file belongs to.** The Settings list shows extensions, so "PNG off" means "*.png
  files". `MediaRecord.format` stays the format of the extension (what the Format column shows and sorts by).
* **The content decides image or video** for containers both can use. `MediaRecord.media_type` is resolved when a file is
  indexed (`MetadataExtractor::resolve_media_type`, 1 KB header): an AVIF photo called `.mp4` is an image, an MP4 called
  `.avif` a video. The ISO base media `ftyp` brands separate AVIF/HEIC (image) from MP4, QuickTime and 3GPP (video); Matroska
  and WebM by their DocType; Ogg is video only with a Theora/OGM stream; ASF is video only with a video stream (a `.wma`
  has none); RIFF by its form type. Everything that decides image or video afterwards (thumbnails, viewer, regeneration,
  rename, clipboard) uses the record, never the extension.
* Animated pictures (GIF, WebP, APNG, AVIF sequences) are images with hover previews and an ANIM badge (see "Animated pictures").
* `.ts` is also TypeScript and `.m2ts`/`.mts` could be anything: formats flagged `needs_header_check` are only taken when the
  header has the transport stream signature (four sync bytes one packet apart, and not plain text). The scan and the tree
  walk count what was skipped as "header does not match".
* Renaming a file changes its format (extension) but never image/video: a JPEG renamed to `.mp4` stays an image.

## Switching formats off

Settings > File Formats lists every format with a switch, its extensions, decoder, notes and how many indexed files it
owns (database only, read on a worker thread when the page opens), a search field (name, id, extension), "All on / All off"
per section (they act on what the search shows) and an overview of what RustView cannot show. The settings file stores the
**disabled ids** (`formats.disabled_formats`), so every format is on by default, formats added later are on, and unknown ids
are dropped on load.

A switched-off format is *skipped, never removed*:

| Where | Effect |
|---|---|
| Scan (`DirectoryScanner`) | its files are not listed, so they are not indexed, hashed or thumbnailed |
| Refresh (`FolderRefresher`) | records of the format are left exactly as they are: not compared, not marked missing, kept in the folder's list |
| Tree walk (`folder_walker`) | files of the format are not counted |
| Auto-extension | an extensionless file of the format is not renamed |
| Lists, queries, counts | `BridgeMedia::visible_unique_by_id` (every list the UI builds), `MediaRepository` (`get_by_folder`, `get_active_count`, `search`) leave the records out; status bar, tag counts and the thumbnail pipeline follow the list |
| Folder counts of the tree | `FolderFormatCounts::apply_change` moves them by exactly the number of hidden/restored records |
| Library, tags, ratings, thumbnails, GC | untouched; switching the format on brings everything back |
| Open tabs | stay open |

Applying a change (`ui/formats_policy.rs`): settings saved -> `FormatPolicy` replaced (`SettingsManager`) -> worker thread
adjusts the folder counts -> tree and the open list (folder, recursive view or tag query) are rebuilt from the library.
Files of a newly switched-on format that were never indexed appear with the next refresh (the app never reads the drive
on its own); the status line says so.

Folder counts are changed by differences, not recomputed from the library: a folder whose files were never indexed keeps the
count of its tree scan, and switching a format off and on again returns every count to where it was. A per-folder refresh now
also sets the folder's count to the number of records it lists (before, only the tree scan and file operations did), so the
tree and the list agree after a refresh; rename into/out of a switched-off format and deleting a folder adjust it too.

Transparency after a refresh: the status line names what was left out (`IgnoredFiles::describe`), e.g.
`Ready. Skipped: 11 not supported (7 .cr2, 4 .nef); 2 of switched-off formats (2 PNG); 1 whose content does not match their
extension.` Only media-like extensions are listed (`KNOWN_UNSUPPORTED_EXTENSIONS`: camera RAW and a few others), `.txt` and
friends are not mentioned.

## Decoding

* **Thumbnails** (`thumbnails/image.rs`): SVG (resvg) -> `image` crate (extension first, content overrides: TGA has no
  signature) -> tile grid (FFmpeg stream group) -> FFmpeg first frame. Videos: FFmpeg or mpv.
* **Viewer** (`viewer/loader.rs`): the same chain at the picture's own size (the FFmpeg fallback used to scale small
  pictures up to 8192 px: a 480x320 AVIF became 134 MB); SVG is rendered at 2048-4096 px on its long edge so it stays sharp.
* **Metadata**: dimensions from the `image` header or SVG size, grid size or FFmpeg; videos from FFmpeg with the stored
  rotation.
* **SVG** goes through resvg, not FFmpeg's librsvg (which shows a file without `width`/`height` at 100x100 and ignores its
  `viewBox`). Transparent areas are white; files an SVG references are never loaded.
* **Tile grids** (`tile_grid.rs`): HEIC photos and some AVIF files are many small pictures plus a "tile grid" stream group
  that only the `ffmpeg` command composes. The module decodes every tile, places it, crops to the picture and applies the
  rotation.
* **Rotation** (`orientation.rs`): the display matrix of a stream (phone videos, `irot` of HEIF/AVIF) is applied to
  thumbnails and swaps width and height in the metadata. The old `rotate` tag is only a fallback: current FFmpeg files
  carry no such tag, so portrait phone videos used to get landscape thumbnails and dimensions. Existing records of such
  videos keep their old dimensions until their next refresh. Mirroring (`imir`) is ignored.
* The thumbnailer reads a short stream from its start without seeking: a seek to 0 loses streams whose first keyframe lies
  after 0 (FLV, raw H.264), which gave black thumbnails.

## Animated pictures

`thumbnails/animated_frames.rs` does for GIF, APNG, animated WebP (`image` crate frame streams) and AVIF sequences (FFmpeg, the
stream with the most frames) what the thumbnailer does for videos: once, while the pipeline reads the file for the thumbnail, it
samples `preview_frame_count` frames evenly, scales them to thumbnail size and stores them compressed with the preview format and
quality of Settings > Performance in the pack store (`VideoPreviewRecord` in `video_previews`, no schema change). A record without
frames marks a picture that was checked and is not animated. The hover reads only the pack store, with the uniform cadence of video
previews, and never opens the picture file; no whole animation is kept in the database or in RAM. At most 400 source frames are read.
* New pictures get their frames with their thumbnail. Pictures indexed before get them only from a refresh the user starts; until then
  nothing changes (a GIF without stored frames still decodes its file on hover, as before). Static PNG/WebP are checked once.
* The ANIM badge (Settings > Appearance & Grid) is shown for pictures with stored frames; the flag is read from the database when a
  folder loads (animatable pictures only) and set when the pipeline stores frames.
* Needs `generate_video_previews` (Settings > Performance) like video previews.

## Not supported

These are not handled:

* **Camera RAW** (.cr2 .cr3 .nef .arw .dng .orf .rw2 .raf .pef .srw ...): needs a RAW decoder (`rawler`/`libraw`).
* **Layered / document sources** (.xcf .kra .ai .eps .pdf), **vector metafiles** (.wmf .emf), **icons** `.icns`/`.cur`.
* **JPEG XR** (.jxr .wdp), **BPG**, **FLIF**, **MNG**: no decoder in the libraries RustView uses.
* **Multi-page** TIFF beyond the first page, **PSB**.
* **Audio-only containers**: an audio-only .mp4/.mkv/.webm is indexed and marked as damaged (no video stream).
* HEIC/AVIF **HDR gain maps, depth maps, alpha channels** are ignored; **EXIF orientation of JPEGs** is not applied
  (a portrait phone photo shows rotated; existing behavior, not part of this work).
* **Colors**: videos and HEIC are converted YUV -> RGB with FFmpeg's default matrix (BT.601), no ICC conversion.
* **PSD**: only the flattened composite; an RLE-compressed PSD written by ImageMagick fails in FFmpeg ("Invalid rle char"),
  files from Photoshop were not available to test.

## Verification

Local test media tree (`Formats/`, generated by `bin/gen_media.py`, 130+ files): one labeled sample
per format and extension, the awkward cases (wrong extensions, extensionless files, text that looks like headers,
TypeScript `.ts`, audio-only containers, HEIC/AVIF grids and rotation, a rotated phone video, unsupported formats).

Every sample was run through the real metadata extractor, thumbnailer and viewer loader (a probe test and unit tests, removed
at finalization, see git history of branch `claude`) and the thumbnails were inspected as contact sheets; videos were also played with mpv
(`--vo=null --frames=2`) and the app was driven on the X11 session of the test machine (switch a format off and on, restart, rename,
refresh, auto-extension with a format switched off, viewer tabs for AVIF/SVG/TS, mpv log).

Results: all 28 image and 22 video formats decode, produce correct dimensions and thumbnails, and play; the `Formats/`
images that depend on features are correct (AVIF grid 480x320 composed from four tiles, HEIC grids with edge tiles
1280x800 and 1920x1080, irot 90 shows 320x480, SVG without width/height 480x320, TGA/WBMP/FF found by extension, raw
H.264/HEVC thumbnails). Known failures that are not format support: audio-only video containers (damaged by design),
RLE PSD from ImageMagick.

Policy checks (unit tests, removed at finalization): a refresh keeps
records of a switched-off format and does not mark them missing, counts move with the switches and return exactly, rename
into a switched-off format moves the count, repository reads and searches skip hidden formats, the format table never
disagrees with the stored enum order, `.ts` sources are not taken.


## Editing (crop and save)

Which pictures the editor can crop and how they are saved (decided by `rustview_core::image_edit::formats::save_support` from the content of the file, not
its extension; checked against every sample of the test environment by `crates/rustview-core/tests/image_edit_formats.rs`):

| Saved over the original | How |
|---|---|
| JPEG (`.jpg .jpeg .jpe .jfif .jif`, also CMYK and grayscale) | lossless when the crop starts on the 8/16 px block grid (CMYK always), else re-encoded with the file's own quantization tables and sampling; EXIF/ICC/XMP kept |
| PNG (8/16 bit, palette, alpha) | lossless; a palette PNG keeps palette, transparency table and bit depth; ancillary chunks kept |
| WebP (still) | lossless stays lossless; lossy uses the WebP preset quality; ICC/EXIF/XMP kept |
| AVIF (opaque, 8 bit) | re-encoded with the AVIF presets (rav1e) |
| TIFF (single page, 8/16 bit) | lossless with the TIFF preset compression; ICC and XMP kept |
| BMP/DIB, still GIF (palette kept), QOI, TGA, Farbfeld | lossless |

| Not overwritten | |
|---|---|
| HEIC/HEIF, SVG, PSD, JPEG XL, JPEG 2000, EXR, HDR, DDS, ICO, PNM family, DPX, PCX, SGI, Sun Raster, XBM, XPM, XWD, WBMP, AVIF with alpha or more than 8 bits | Save opens the export dialog (a new file) |
| Animated GIF/APNG/WebP, AVIF sequences, multi-page TIFF | cannot be cropped (the editor shows only the first frame) |
