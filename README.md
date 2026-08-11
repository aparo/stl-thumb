# stl-thumb

[![Build Status](https://github.com/neosmith20/stl-thumb/workflows/Build/badge.svg)](https://github.com/neosmith20/stl-thumb/actions/workflows/build-ci.yml)
[![Documentation](https://img.shields.io/docsrs/stl-thumb/latest)](https://docs.rs/stl-thumb/latest/stl_thumb/)
[![Crates.io](https://img.shields.io/crates/v/stl-thumb.svg)](https://crates.io/crates/stl-thumb)

Stl-thumb is a fast lightweight thumbnail generator for 3D model (STL, OBJ, 3MF) files. It can show previews for model files in your file manager on Linux and Windows. It is written in Rust and uses [wgpu](https://wgpu.rs/) for cross-platform GPU rendering.

![Screenshot](https://user-images.githubusercontent.com/3131268/116009182-f3f89c80-a5cc-11eb-817d-91e8a9fad279.png)

> This is a fork of [unlimitedbacon/stl-thumb](https://github.com/unlimitedbacon/stl-thumb) updated to use glium 0.35 and modern dependencies.

## Installation

### Windows

Stl-thumb requires 64 bit Windows 7 or later.

[Download the latest `windows-x64` artifact](https://github.com/neosmith20/stl-thumb/actions) from the Actions tab and extract the `.exe`.

To get automatic thumbnails in Windows Explorer, first install the shell extension from the [original STLThumbWindows installer](https://github.com/unlimitedbacon/stl-thumb/releases/latest), then replace `stl-thumb.exe` in the install directory (usually `C:\Program Files\stl-thumb\`) with the one from this fork.

The installer will tell the Windows shell to refresh the thumbnail cache, however this does not always seem to work. If your icons do not change then try using the [Disk Cleanup](https://en.wikipedia.org/wiki/Disk_Cleanup) utility to clear the thumbnail cache.

### Linux

Stl-thumb works with Gnome and most other similar desktop environments. If you are using the KDE desktop environment then you will also need to install the separate [`stl-thumb-kde`](https://github.com/unlimitedbacon/stl-thumb-kde) package.

Make sure that your file manager is set to generate previews for files larger than 1 MB. Most file managers have this setting under the Preview tab in their Preferences.

#### Debian / Ubuntu

[Download the latest `linux-amd64-packages` artifact](https://github.com/neosmith20/stl-thumb/actions) from the Actions tab, extract the `.deb` file, and install it:

```
$ sudo apt install ./stl-thumb_0.5.0_amd64.deb
```

#### Arch

A package is available [in the AUR](https://aur.archlinux.org/packages/stl-thumb/). Note the AUR package tracks the upstream release, not this fork.

```
$ yay -S stl-thumb
```

#### openSUSE

For openSUSE Tumbleweed there is a user repo available:

```
$ sudo zypper ar -f obs://home:jubalh:stl stl
$ sudo zypper ref
$ sudo zypper install stl-thumb
```

## Building

### Requirements

- Rust (stable)
- `libfontconfig-dev` (Linux)

### Building the tool itself:

```
$ cargo build --release
```

### Building the .deb package:

```
$ cargo install cargo-deb
$ cargo deb
```

### Building the .rpm package:

```
$ cargo install cargo-generate-rpm
$ cargo generate-rpm
```

## Command Line Usage

```
$ stl-thumb <MODEL_FILE> [IMG_FILE]
```

### Options

| Option | Description |
| --- | --- |
| \<MODEL_FILE\> | The model file you want a picture of. Use `-` to read from stdin instead of a file. |
| \<IMG_FILE\> | The thumbnail image file that will be created. Use `-` to write to stdout instead of a file. |
| -s, --size \<size\> | Specify the width of the image in pixels. The image is always square. |
| -f, --format \<format\> | The format of the image file. If not specified it will be determined from the file extension, or default to PNG if there is no extension. Supported formats: PNG, JPEG, GIF, ICO, BMP |
| -m, --material \<ambient\> \<diffuse\> \<specular\> | Colors for rendering the mesh using the Phong reflection model. Requires 3 colors as rgb hex values: ambient, diffuse, and specular. Defaults to blue. |
| -b, --background \<color\> | The background color with transparency (rgba). Default is `ffffff00` (transparent). |
| -a, --antialiasing [none, fxaa] | Anti-aliasing method. Default is FXAA, which is fast but may introduce artifacts. |
| --recalc-normals | Force recalculation of face normals. Use when dealing with malformed STL files. |
| -w, --multi-view | Generate a 2×2 grid with four standard views: isometric, front, top, and side. The default output size is twice the normal default so each tile retains full resolution. |
| -l, --label | Draw a view-name label (Isometric, Front, Top, Side) at the top of each panel in the multi-view grid. Requires `--multi-view`. |
| -x | Display the image in a window instead of saving a file. |
| -h, --help | Prints help information. |
| -V, --version | Prints version information. |
| -v[v][v] | Increase message verbosity. Levels: Errors, Warnings, Info, Debugging |

### Multi-view example

```
$ stl-thumb model.stl thumb.png --multi-view --label
$ stl-thumb model.stl thumb.png -w -l -s 1024
```

The `-w` flag renders four camera angles and stitches them into a single image:

```
┌─────────────┬─────────────┐
│  Isometric  │    Front    │
├─────────────┼─────────────┤
│     Top     │    Side     │
└─────────────┴─────────────┘
```

A 2-pixel separator line is always drawn between panels. Adding `-l` overlays the view name at the top of each panel.

## Changes from upstream

- Migrated GPU backend from glium/OpenGL to [wgpu](https://wgpu.rs/) 30.0 with WGSL shaders
- Added `--multi-view` (`-w`) flag: renders a 2×2 grid of isometric, front, top, and side views
- Added `--label` (`-l`) flag: overlays view-name labels on each panel of the multi-view grid
- Updated winit, clap, image, and other dependencies to current versions
- Modernized GitHub Actions CI workflow with Node.js 24
- Removed deprecated Travis CI and AppVeyor configuration
- Added Windows x64 build artifact to CI
