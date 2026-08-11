#[macro_use]
extern crate log;
extern crate stderrlog;

extern crate stl_thumb;

use std::process;
use stl_thumb::config::Config;
use stl_thumb::info::ModelInfo;

fn main() {
    let config = Config::new();

    stderrlog::new()
        .module(module_path!())
        .verbosity(config.verbosity)
        .init()
        .unwrap();

    info!("MODEL File: {}", config.model_filename);
    info!("IMG File: {}", config.img_filename);

    if config.info {
        match ModelInfo::from_file(&config.model_filename, config.recalc_normals) {
            Ok(kpi) => print!("{}", kpi),
            Err(e) => {
                error!("Failed to extract model info: {}", e);
                process::exit(1);
            }
        }
        // If no IMG_FILE was given, info-only mode — exit here.
        if config.img_filename.is_empty() {
            return;
        }
    }

    if config.visible {
        if let Err(e) = stl_thumb::render_to_window(config) {
            error!("Application error: {}", e);
            process::exit(1);
        }
    } else if !config.img_filename.is_empty() {
        if let Err(e) = stl_thumb::render_to_file(&config) {
            error!("Application error: {}", e);
            process::exit(1);
        }
    }
}

// Notes
// =====
//
// Linux Thumbnails
// ----------------
// https://tecnocode.co.uk/2013/10/21/writing-a-gnome-thumbnailer/
// https://wiki.archlinux.org/index.php/XDG_MIME_Applications#Shared_MIME_database
// https://developer.gnome.org/integration-guide/stable/thumbnailer.html.en (outdated)
//
// Window Thumbnails
// -----------------
// https://code.msdn.microsoft.com/windowsapps/CppShellExtThumbnailHandler-32399b35
// https://github.com/Arlorean/Voxels
//
// Helpful Examples
// ----------------
// https://github.com/bwasty/gltf-viewer
//
// OpenGL
// ------
// https://glium-doc.github.io/#/
// http://www.opengl-tutorial.org/beginners-tutorials/tutorial-3-matrices/
