# Third-party notices

The application selects the MIT license for each dependency in its active Rust
dependency tree. The lockfile pins these versions:

| Package | Version | Upstream license | Selected license |
| --- | --- | --- | --- |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | MIT |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | MIT |
| itoa | 1.0.19 | MIT OR Apache-2.0 | MIT |
| memchr | 2.8.3 | Unlicense OR MIT | MIT |
| zmij | 1.0.23 | MIT | MIT |

License text below is copied from the downloaded crates. Preserve this document
with redistributed source or binaries. `cargo tree --locked` shows the active
dependencies; the MIT licensing check runs as part of `cargo test --locked`.

GitHub CLI is an independently installed runtime prerequisite, not a bundled
component. Its project is [MIT licensed](https://github.com/cli/cli/blob/trunk/LICENSE).
There are no JavaScript libraries, external fonts, images, or icon packages.

## serde_json, serde_core, itoa, and zmij

These four crates include the following MIT permission notice:

```text
Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
```

## memchr

```text
The MIT License (MIT)

Copyright (c) 2015 Andrew Gallant

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
```
