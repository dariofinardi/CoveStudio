# onnxruntime — vendored runtime DLLs

This directory holds the **ONNX Runtime** native libraries that the
`ort` crate loads at runtime (we build with `ort/load-dynamic`, NOT
statically linked, NOT relying on a system installation). Drop the
right DLL/`.so`/`.dylib` for your target into the matching subfolder
and the embedding service will find it via the discovery walk in
[`src/embeddings/service.rs::ensure_onnxruntime_dylib_path`].

```
libs/onnxruntime/
├── win-x64/         onnxruntime.dll
├── win-arm64/       onnxruntime.dll
├── linux-x64/       libonnxruntime.so
├── linux-aarch64/   libonnxruntime.so
├── macos-x64/       libonnxruntime.dylib
└── macos-arm64/     libonnxruntime.dylib
```

The libraries themselves are **never committed to git** — see the
`*.dll` / `*.so` / `*.dylib` rules in the top-level `.gitignore`. Each
contributor / build runner fetches the variant they need for their
machine and target.

## Why local rather than system-installed?

- **Reproducibility.** The runtime version we test against is the
  one we ship. A user's `system32` may carry a different build with
  subtly different EP behaviour.
- **Sovereignty.** A sovereign-by-default product can't depend on
  whatever `onnxruntime.dll` happened to be installed by some
  upstream Windows update or NVIDIA toolchain.
- **Security.** No `LoadLibrary` against `system32` means no DLL-
  hijack surface where a poisoned `onnxruntime.dll` on the search
  path could be loaded before ours.

## Version: must be exactly 1.20.0 for ort 2.0.0-rc.9

`ort 2.0.0-rc.9` (paired with `fastembed 4.9.1`) compiles against
onnxruntime **1.20.0**. The vendored DLL must match that minor version
*exactly* — anything else (older OR newer, including Microsoft's stock
1.24.x releases) deadlocks `TextEmbedding::try_new_from_user_defined`
silently because ort's function-pointer table references symbols that
do not match the loaded DLL's ABI. Verify what version ort-sys actually
links with:

```powershell
# Strings like "branch=rel-1.20.0, git-commit=..." in cove-studio.exe
# reveal the version ort-sys compiled against. Match the vendored
# DLL to whatever that string says.
Select-String -Path target\debug\cove-studio.exe `
  -Pattern 'branch=rel-\d+\.\d+\.\d+' -Encoding Default
```

When the `ort` dep changes, re-run the strings probe and re-vendor the
DLLs to match — don't trust the official onnxruntime releases page to
be "close enough".

### Why we are deliberately on the rc.9 line

The downgrade from `ort 2.0.0-rc.12` (paired with onnxruntime 1.20.0)
back to rc.9 (1.20.0) is preparatory to a new data-security / privacy
feature: it relies on the smaller, audited 1.20.0 runtime surface
and on the execution-provider set that ships with it. Side effects of
the downgrade:

- The `rag-webgpu` and `rag-azure` cargo features are **gone**. Both
  EPs were added to onnxruntime after 1.20.0 — they will return when
  the privacy feature lands and we can move the runtime back forward.
- The Vitis EP type is now spelled `VitisAIExecutionProvider` (rc.9
  uses the long-form names; rc.12 had short `Vitis` aliases).
- `UserDefinedEmbeddingModel` is now `#[non_exhaustive]` and built
  through `::new(...).with_pooling(...).with_quantization(...)` — the
  `external_initializers` / `output_key` fields that fastembed 5.x
  exposes do not exist on this version.

See [`HISTORY.md`](../../HISTORY.md) (2026-05-20) for the full story.

## Which variant?

ONNX Runtime ships several builds, each compiled with a different
set of execution providers (EPs). Pick the one that matches the
hardware on the deployment machine — every EP enabled in the Rust
build with `--features rag-<ep>` will be registered at session-init
time and silently skipped if the loaded DLL doesn't actually support
it. So you can build the binary with `rag-accel-all` and swap the
DLL freely depending on the host.

| Machine class | Recommended onnxruntime build | Bundle to download |
|---|---|---|
| **Windows desktop, any DX12 GPU** | DirectML | `Microsoft.ML.OnnxRuntime.DirectML` 1.20.0 (NuGet) — onnxruntime.dll + DirectML.dll |
| **Windows + NVIDIA RTX/GeForce** | CUDA + TensorRT | `onnxruntime-win-x64-gpu-1.20.0.zip` (GitHub releases) |
| **Windows + Intel CPU/iGPU** | OpenVINO | `Microsoft.ML.OnnxRuntime.OpenVino` 1.20.0 |
| **Windows ARM64 (Snapdragon X Elite)** | QNN + DirectML | `Microsoft.ML.OnnxRuntime.QNN` 1.20.0 |
| **Linux + NVIDIA** | CUDA + TensorRT | `onnxruntime-linux-x64-gpu-1.20.0.tgz` |
| **Linux + AMD** | ROCm | `onnxruntime-linux-x64-rocm-1.20.0.tgz` |
| **macOS Apple Silicon** | CoreML | `onnxruntime-osx-arm64-1.20.0.tgz` |
| **macOS Intel** | CPU | `onnxruntime-osx-x86_64-1.20.0.tgz` |
| **CPU-only / portable** | base CPU | `onnxruntime-<os>-<arch>-1.20.0.{zip,tgz}` |

## Fetching (Windows quick path)

```powershell
# Pick the right URL from https://github.com/microsoft/onnxruntime/releases
# v1.20.0 is what ort 2.0.0-rc.9 expects — bumping ort means
# bumping this too (see "Version" note above).
$ver = "1.20.0"
$arch = "x64"   # or "arm64"
$url = "https://github.com/microsoft/onnxruntime/releases/download/v$ver/onnxruntime-win-$arch-$ver.zip"
Invoke-WebRequest -Uri $url -OutFile "$env:TEMP\ort.zip"
Expand-Archive "$env:TEMP\ort.zip" -DestinationPath "$env:TEMP\ort"
Copy-Item "$env:TEMP\ort\onnxruntime-win-$arch-$ver\lib\onnxruntime.dll" `
          -Destination ".\libs\onnxruntime\win-$arch\"
```

For DirectML / CUDA / OpenVINO / QNN variants, follow the same flow
with the corresponding NuGet or release artefact. The bundle ships
multiple DLLs (e.g. `onnxruntime.dll` + `DirectML.dll`) — copy *all*
of them into the same subdirectory. The loader picks up
`onnxruntime.dll` by name, but ort calls into the side-DLLs
internally at session time.

## Overriding the search path

Set `ORT_DYLIB_PATH` to an absolute path before the binary starts:

```powershell
$env:ORT_DYLIB_PATH = "C:\path\to\custom\onnxruntime.dll"
```

The `ensure_onnxruntime_dylib_path()` helper respects this and skips
the discovery walk. Useful for CI runners with a shared cache, or
for testing a specific build without copying it in-tree.

## When the DLL is missing

The service logs a warning at startup and the first embed call
fails with a clear "load library failed" error. The user-facing
catch is intentional: a silent fallback to a system DLL or to
"download on first run" would defeat the sovereignty guarantee
above.
