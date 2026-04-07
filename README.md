# stl-thumb

[![Build Status](https://github.com/neosmith20/stl-thumb/workflows/Build/badge.svg)](https://github.com/neosmith20/stl-thumb/actions/workflows/build-ci.yml)
[![Documentation](https://img.shields.io/docsrs/stl-thumb/latest)](https://docs.rs/stl-thumb/latest/stl_thumb/)
[![Crates.io](https://img.shields.io/crates/v/stl-thumb.svg)](https://crates.io/crates/stl-thumb)

Stl-thumb is a fast lightweight thumbnail generator for 3D model (STL, OBJ, 3MF) files. It can show previews for model files in your file manager on Linux and Windows. It is written in Rust and uses OpenGL.

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
- `libosmesa6-dev` (Linux, for headless rendering)

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
| \<MODEL_FILE\> | The model file you want a picture of. Use - to read from stdin instead of a file. |
| \<IMG_FILE\> | The thumbnail image file that will be created. Use - to write to stdout instead of a file. |
| -s, --size \<size\> | Specify width of the image. It will always be a square. |
| -f, --format \<format\> | The format of the image file. If not specified it will be determined from the file extension, or default to PNG if there is no extension. Supported formats: PNG, JPEG, GIF, ICO, BMP |
| -m, --material \<ambient\> \<diffuse\> \<specular\> | Colors for rendering the mesh using the Phong reflection model. Requires 3 colors as rgb hex values: ambient, diffuse, and specular. Defaults to blue. |
| -b, --background \<color\> | The background color with transparency (rgba). Default is ffffff00. |
| -a, --antialiasing [none, fxaa] | Anti-aliasing method. Default is FXAA, which is fast but may introduce artifacts. |
| --recalc-normals | Force recalculation of face normals. Use when dealing with malformed STL files. |
| -x | Display the image in a window instead of saving a file. |
| -h, --help | Prints help information. |
| -V, --version | Prints version information. |
| -v[v][v] | Increase message verbosity. Levels: Errors, Warnings, Info, Debugging |

## Changes from upstream

- Updated glium from 0.32 to 0.35
- Updated winit, clap, image, and other dependencies to current versions
- Modernized GitHub Actions CI workflow with Node.js 24
- Removed deprecated Travis CI and AppVeyor configuration
- Added Windows x64 build artifact to CI
```

The main changes from the original are pointing the badge and download links to your fork, removing the dead AppVeyor badge, fixing the typo in "background", adding the note that it's a fork, and documenting the Windows install process correctly.
