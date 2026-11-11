use std::fs;
use std::path::PathBuf;

pub fn is_enabled() -> bool {
    path().is_some_and(|path| path.is_file())
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let path = path().ok_or_else(|| "autostart is unsupported".to_owned())?;
    if enabled {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let body = launch_body()?;
        fs::write(&path, body).map_err(|error| error.to_string())?;
    } else if path.is_file() {
        fs::remove_file(&path).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = directories::UserDirs::new()?.home_dir().to_path_buf();
        Some(
            home.join("Library/LaunchAgents")
                .join(format!("{}.plist", crate::brand::bundle_id())),
        )
    }
    #[cfg(target_os = "linux")]
    {
        let home = directories::UserDirs::new()?.home_dir().to_path_buf();
        Some(
            home.join(".config/autostart")
                .join(format!("{}.desktop", crate::brand::url_scheme())),
        )
    }
    #[cfg(target_os = "windows")]
    {
        let base = directories::BaseDirs::new()?.data_dir().to_path_buf();
        Some(
            base.join("Microsoft/Windows/Start Menu/Programs/Startup")
                .join(format!("{}.cmd", crate::brand::url_scheme())),
        )
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows"
    )))]
    {
        None
    }
}

fn launch_body() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let exe = exe.display().to_string();
    #[cfg(target_os = "macos")]
    {
        Ok(format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
</dict>
</plist>
"#,
            crate::brand::bundle_id(),
            xml_escape(&exe),
        ))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(format!(
            "[Desktop Entry]\nType=Application\nName={}\nExec=\"{}\"\nX-GNOME-Autostart-enabled=true\n",
            crate::brand::product(),
            exe.replace('"', "\\\""),
        ))
    }
    #[cfg(target_os = "windows")]
    {
        Ok(format!("@echo off\r\nstart \"\" \"{}\"\r\n", exe.replace('"', "")))
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows"
    )))]
    {
        let _ = exe;
        Err("autostart is unsupported".into())
    }
}

#[cfg(target_os = "macos")]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
