//! Software-renderer fallback shared by both startup branches.

use std::process::Command;

const BACKEND_ENV: &str = "SLINT_BACKEND";
const SOFTWARE_BACKEND: &str = "winit-software";

/// Substrings that identify a GPU/GL failure in a `PlatformError`.
///
/// `PlatformError` has no variant for this - the winit backend reports every
/// renderer failure as `Other(String)` ("Failed to initialize OpenGL driver",
/// "Cannot create OpenGL context: ...", "FemtoVG: Error making context
/// current: ..."), so the message is the only thing left to match on. Software
/// rendering fails with "softbuffer" messages instead, which deliberately do
/// not match: retrying those in software mode would be pointless.
const RENDERER_FAILURES: [&str; 5] = ["opengl", "glutin", "femtovg", "skia", "glcreateshader"];

fn is_renderer_failure(err: &slint::PlatformError) -> bool {
    let message = err.to_string().to_lowercase();
    RENDERER_FAILURES
        .iter()
        .any(|needle| message.contains(needle))
}

pub fn force_software_backend() {
    std::env::set_var(BACKEND_ENV, SOFTWARE_BACKEND);
}

fn backend_pinned() -> bool {
    std::env::var(BACKEND_ENV).is_ok_and(|value| !value.is_empty())
}

/// Turns an event-loop result into the process's result, retrying once with the
/// software renderer when the default one turns out to be unusable.
///
/// Slint creates the renderer lazily when the event loop shows the first
/// window, so a dead GL driver surfaces from the loop and not from
/// `AppWindow::new` - which is why [`create_window`](crate::create_window)
/// alone never catches it. The platform backend is cached in a thread-local
/// `OnceCell` that cannot be replaced once set, so `SLINT_BACKEND` is not
/// re-read in this process and the retry has to be a child process.
///
/// The retry child is a second instance of this app, so the caller must first
/// drop every OS-level singleton it holds - the global hotkey registration and
/// the tray icon - or the child fails to register the same hotkey while the
/// parent blocks on it. The parent also stays alive until the child exits, so
/// this takes an already-computed `committed` rather than a closure: nothing
/// may still be running that could change the answer.
///
/// `committed` reports whether the process has already done something a restart
/// would repeat - tray mode launches the trainer before the loop starts when no
/// hotkey is configured. A committed process reports the failure instead of
/// restarting, so a trainer is never launched twice.
pub fn recover(
    outcome: Result<(), slint::PlatformError>,
    committed: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let err = match outcome {
        Ok(()) => return Ok(()),
        Err(err) => err,
    };

    if !is_renderer_failure(&err) || committed || backend_pinned() {
        return Err(err.into());
    }

    eprintln!("Renderer failed to initialize: {err}");
    eprintln!("Restarting with the software renderer ({SOFTWARE_BACKEND})...");

    let status = Command::new(std::env::current_exe()?)
        .args(std::env::args_os().skip(1))
        .env(BACKEND_ENV, SOFTWARE_BACKEND)
        .status()?;

    std::process::exit(status.code().unwrap_or(1));
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::PlatformError;

    /// Verbatim messages from i-slint-backend-winit's femtovg renderer.
    #[test]
    fn recognizes_the_gl_failures_the_winit_backend_reports() {
        for message in [
            "Failed to initialize OpenGL driver: Could not locate glCreateShader symbol",
            "Cannot create OpenGL context: NotSupported(\"no available pixel format\")",
            "Error creating OpenGL Window surface: BadDisplay",
            "Error finalizing window for OpenGL rendering: os error",
            "FemtoVG Renderer: Failed to make newly created OpenGL context current: Misc",
            "FemtoVG: Error making context current: ContextLost",
        ] {
            assert!(
                is_renderer_failure(&PlatformError::Other(message.into())),
                "should have matched: {message}"
            );
        }
    }

    #[test]
    fn ignores_software_renderer_and_unrelated_failures() {
        for message in [
            "Error creating softbuffer context: unsupported",
            "Error retrieving softbuffer rendering buffer: out of memory",
            "no trainer folder is configured",
        ] {
            assert!(
                !is_renderer_failure(&PlatformError::Other(message.into())),
                "should not have matched: {message}"
            );
        }
        assert!(!is_renderer_failure(&PlatformError::NoPlatform));
    }
}
