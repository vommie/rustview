# Attribution

Only what the licenses require when a built RustView is passed on. `scripts/attribution.py` writes this file.

RustView's own source code is under the MIT License, see `LICENSE`. The built program as a whole is under the GPL-3.0-only, because
it links the libmpv and FFmpeg libraries of the system it is built on, which are usually built under the GPL (Arch and Manjaro do);
its source is at <https://github.com/vommie/rustview>.
Long license texts stand once in `LICENSES/`.

## Compiled-in C and C++ code

- **Skia** by Google (BSD-3-Clause, copyright line and text below), compiled in by the Slint renderer through `skia-bindings`, with:
  - **libjpeg-turbo**: this software is based in part on the work of the Independent JPEG Group.
  - **Expat** (MIT, copyright lines below).
  - **Wuffs** by Google, under the Apache-2.0 (`LICENSES/Apache-2.0.txt`).
  - libpng and zlib, whose licenses ask nothing of a binary.
- **libwebp** by Google (BSD-3-Clause, copyright line and text below), compiled in through `libwebp-sys`.
- **LMDB** by Howard Chu, Symas Corp. and the OpenLDAP Foundation, compiled in through `lmdb-master-sys`, under the OpenLDAP Public
  License 2.8 (copyright lines and text below); its `mdb.c` holds code by Martin Hedenfalk under the ISC License (below).
- **SQLite**, compiled in through `libsqlite3-sys`, is in the public domain.
- **Tabler Icons**, MIT, copyright line and text below.
