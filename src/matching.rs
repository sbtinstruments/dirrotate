use log::info;
use path_matchers::{glob, PathMatcher};
use std::path::{Path, PathBuf};

fn canonicalize_pattern(base_dir: &Path, pattern: &String) -> String {
    let mut res = String::from(base_dir.to_str().expect("Base dir not valid Unicode."));
    res.push('/');
    res.push_str(pattern.as_str());
    info!("Using a matching pattern: {}", res);
    res
}

pub fn get_path_matcher(base_dir: &PathBuf, pattern: &Option<String>) -> Option<impl PathMatcher> {
    pattern
        .as_ref()
        .map(|p| canonicalize_pattern(&base_dir, &p))
        .map(|pattern| glob(&pattern).expect("Not a valid glob pattern"))
}
