//! `uml-viewer scene --frame`: the scene frame of one view as JSON (#16).
//!
//! ```text
//! uml-viewer scene --frame --ir <file.edn> [--focus <edn>] [--metrics-root <dir>]
//!                  [--wide [--aspect <w/h>]] [--extra <edn map>]
//!                  [--width <pt>] [--height <pt>] [--zoom <z> --camera <x>,<y>]
//!                  [--select <edn hit>] [--hover <edn hit>] [--pointer <x>,<y>]
//!                  [--hover-metadata] [--lang <tag>] [--output <file>]
//! ```
//!
//! Without `--frame` the arguments go to the scene geometry export
//! unchanged (`scene::run`). The view is fitted to the viewport, as the
//! window shows a freshly opened diagram, unless `--zoom` places the
//! camera. A hit is the EDN map hit testing yields, e.g.
//! `{:kind :class :id :ir.graph}`. `--lang` is the language the resident
//! lays text out in (`zh`, else English, the default).

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use edn::Value;
use scene::events::{self, Dims};

use crate::strings::Lang;

const USAGE: &str = "usage: uml-viewer scene --frame --ir <file.edn> [--focus <edn>] \
[--metrics-root <dir>] [--wide [--aspect <w/h>]] [--extra <edn map>] [--width <pt>] \
[--height <pt>] [--zoom <z> --camera <x>,<y>] [--select <edn hit>] [--hover <edn hit>] \
[--pointer <x>,<y>] [--hover-metadata] [--lang <tag>] [--output <file>]";

const EX_USAGE: u8 = 2;
const EX_FAILURE: u8 = 1;

/// The default canvas, in points: the historical 1500x920 window less its
/// chrome and the inspector it once had (`scene::document::WIDE_ASPECT`).
pub const DEFAULT_WIDTH: f64 = 1220.0;
pub const DEFAULT_HEIGHT: f64 = 884.0;

#[derive(Debug)]
struct Options {
    ir: PathBuf,
    focus: Vec<Value>,
    metrics_root: Option<PathBuf>,
    extra: Value,
    width: f64,
    height: f64,
    camera: Option<(f64, f64, f64)>,
    select: Option<Value>,
    hover: Option<Value>,
    pointer: Option<(f64, f64)>,
    /// The optional bounded hover metadata (#26); off for the goldens.
    hover_metadata: bool,
    lang: Lang,
    output: Option<PathBuf>,
}

/// The `scene` role: `--frame` selects the frame, otherwise geometry.
pub fn run_scene(args: &[OsString]) -> ExitCode {
    if !args.iter().any(|a| a == "--frame") {
        return scene::run(args);
    }
    let opts = match parse(args) {
        Ok(opts) => opts,
        Err(message) => {
            eprintln!("uml-viewer scene: {message}\n{USAGE}");
            return ExitCode::from(EX_USAGE);
        }
    };
    match execute(&opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("uml-viewer scene: {message}");
            ExitCode::from(EX_FAILURE)
        }
    }
}

fn number(raw: &str, name: &str) -> Result<f64, String> {
    raw.parse::<f64>()
        .ok()
        .filter(|x| x.is_finite())
        .ok_or_else(|| format!("{name} must be a number"))
}

fn pair(raw: &str, name: &str) -> Result<(f64, f64), String> {
    let (x, y) = raw
        .split_once(',')
        .ok_or_else(|| format!("{name} must be <x>,<y>"))?;
    Ok((number(x.trim(), name)?, number(y.trim(), name)?))
}

fn edn_arg(raw: &str, name: &str) -> Result<Value, String> {
    edn::parse(raw).map_err(|e| format!("{name}: {e}"))
}

fn parse(args: &[OsString]) -> Result<Options, String> {
    let mut ir = None;
    let mut focus = None;
    let mut metrics_root = None;
    let mut wide = false;
    let mut aspect = None;
    let mut extra = Value::Map(vec![]);
    let mut width = DEFAULT_WIDTH;
    let mut height = DEFAULT_HEIGHT;
    let mut zoom = None;
    let mut camera = None;
    let mut select = None;
    let mut hover = None;
    let mut pointer = None;
    let mut hover_metadata = false;
    let mut lang = Lang::default();
    let mut output = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i]
            .to_str()
            .ok_or_else(|| format!("argument is not valid UTF-8: {:?}", args[i]))?;
        let mut next = |name: &str| -> Result<String, String> {
            i += 1;
            args.get(i)
                .and_then(|a| a.to_str())
                .map(str::to_owned)
                .ok_or_else(|| format!("{name} requires a UTF-8 value"))
        };
        match arg {
            "--frame" => {}
            "--ir" => ir = Some(PathBuf::from(next(arg)?)),
            "--focus" => {
                if focus.is_some() {
                    return Err("--frame takes one --focus".into());
                }
                match edn_arg(&next(arg)?, arg)? {
                    Value::Vector(items) => focus = Some(items),
                    _ => return Err("--focus must be an EDN vector of keywords".into()),
                }
            }
            "--metrics-root" => metrics_root = Some(PathBuf::from(next(arg)?)),
            "--wide" => wide = true,
            "--aspect" => {
                let a = number(&next(arg)?, arg)?;
                if a <= 0.0 {
                    return Err("--aspect must be a positive number".into());
                }
                aspect = Some(a);
            }
            "--extra" => match edn_arg(&next(arg)?, arg)? {
                map @ Value::Map(_) => extra = map,
                _ => return Err("--extra must be an EDN map".into()),
            },
            "--width" => width = number(&next(arg)?, arg)?,
            "--height" => height = number(&next(arg)?, arg)?,
            "--zoom" => zoom = Some(number(&next(arg)?, arg)?),
            "--camera" => camera = Some(pair(&next(arg)?, arg)?),
            "--select" => select = Some(edn_arg(&next(arg)?, arg)?),
            "--hover" => hover = Some(edn_arg(&next(arg)?, arg)?),
            "--pointer" => pointer = Some(pair(&next(arg)?, arg)?),
            "--hover-metadata" => hover_metadata = true,
            "--lang" => lang = Lang::from_tag(&next(arg)?),
            "--output" => output = Some(PathBuf::from(next(arg)?)),
            other => return Err(format!("unknown option {other}")),
        }
        i += 1;
    }
    let ir = ir.ok_or("--ir is required")?;
    if aspect.is_some() && !wide {
        return Err("--aspect requires --wide".into());
    }
    if width <= 0.0 || height <= 0.0 {
        return Err("--width and --height must be positive".into());
    }
    if wide {
        extra.assoc(Value::keyword("wide"), Value::Bool(true));
    }
    if let Some(a) = aspect {
        extra.assoc(Value::keyword("aspect"), Value::Float(a));
    }
    let camera = match (zoom, camera) {
        (Some(z), Some((x, y))) if z > 0.0 => Some((x, y, z)),
        (None, None) => None,
        _ => return Err("--zoom (positive) and --camera go together".into()),
    };
    Ok(Options {
        ir,
        focus: focus.unwrap_or_default(),
        metrics_root,
        extra,
        width,
        height,
        camera,
        select,
        hover,
        pointer,
        hover_metadata,
        lang,
        output,
    })
}

fn execute(opts: &Options) -> Result<(), String> {
    let text = std::fs::read_to_string(&opts.ir)
        .map_err(|e| format!("cannot read {}: {e}", opts.ir.display()))?;
    let doc = scene::ir::read_document(&text)
        .map_err(|e| format!("invalid IR {}: {e}", opts.ir.display()))?;
    let snapshots = opts
        .metrics_root
        .as_deref()
        .map(scene::cli::load_snapshots)
        .unwrap_or_default();
    let mut state = scene::document::new_state(
        doc,
        None,
        Some(&opts.focus),
        Some(&opts.extra),
        &snapshots,
        None,
    );
    match opts.camera {
        Some((x, y, zoom)) => {
            state.cam_x = x;
            state.cam_y = y;
            state.zoom = zoom;
        }
        None => {
            let dims = Dims {
                window_w: opts.width,
                window_h: opts.height,
                ..Dims::default()
            };
            state = events::fit(&state, &dims);
        }
    }
    if let Some(hit) = &opts.select {
        state = events::select(&state, Some(hit));
    }
    state.hover = opts.hover.clone();
    state.pointer = opts.pointer;
    let frame = crate::frame::build(&state, opts.width, opts.height, opts.lang);
    // The goldens are written without the optional capability; --hover-metadata
    // adds the same field the resident sends.
    let json = if opts.hover_metadata {
        frame.to_json_string_with_hover(&state.scene)
    } else {
        frame.to_json_string()
    };
    match &opts.output {
        Some(path) => std::fs::write(path, &json)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?,
        None => print!("{json}"),
    }
    Ok(())
}
