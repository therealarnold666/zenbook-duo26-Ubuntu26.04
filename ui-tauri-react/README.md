# Zenbook Duo Control UI

## Performance monitoring

The control panel samples hardware telemetry once per second and shows a compact
summary in the upper-right corner of the window. The Performance page adds a
60-second utilization chart and detailed CPU, GPU, graphics-memory, and system
memory readings.

- CPU load comes from `/proc/stat`; package temperature prefers `coretemp` or
  `k10temp` and falls back to the matching thermal zone.
- GPU load and resident graphics memory come from DRM client accounting in
  `/proc/*/fdinfo`, including Intel's `xe` cycle counters.
- Dedicated VRAM is used when the DRM driver exposes it. Integrated GPUs are
  labelled as shared memory and use system memory as their capacity.
- GPU temperature is shown only when the kernel exposes a GPU-specific hwmon or
  thermal sensor. Unsupported sensors display `--` instead of a guessed value.

## Build commands

- `npm run build`: full Tauri release build with configured bundles.
- `npm run build:local`: release build without packaging bundles. Faster for local verification.
- `npm run build:frontend`: frontend-only production build.
- `npm run build:rust`: Rust-only release build for the Tauri backend.

## Faster builds

The biggest speed win during iteration is to avoid full packaging unless you need `.deb` or `.rpm`.

- Use `npm run build:local` for local release checks.
- Use `npm run build:frontend` when changing only the UI.
- Use `npm run build:rust` when changing only Rust code.
- Use `npm run dev` during active development.

Cargo already uses multiple cores by default. You can cap or tune it explicitly when useful:

```bash
CARGO_BUILD_JOBS=8 npm run build:local
```

If `sccache` is installed, you can enable Rust compile caching:

```bash
RUSTC_WRAPPER=sccache npm run build:local
```

If `mold` is installed on Linux, you can reduce link time with a local Cargo config. An example is provided in `.cargo/config.toml.example`.

To set up the recommended local tooling automatically:

```bash
./scripts/setup-fast-build.sh
```
