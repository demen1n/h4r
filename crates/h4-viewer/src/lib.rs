use std::path::PathBuf;

/// Game data folder: first argument, else `$H4_DATA`, else the Sikarugir wrapper's copy.
pub fn data_dir(arg: Option<String>) -> PathBuf {
    if let Some(a) = arg.or_else(|| std::env::var("H4_DATA").ok()) {
        return PathBuf::from(a);
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(
        "Applications/Sikarugir/heroes4.app/Contents/SharedSupport/prefix/drive_c/\
         Program Files (x86)/Heroes of Might and Magic IV/data",
    )
}
