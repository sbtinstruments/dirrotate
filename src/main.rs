mod matching;
use clap::Parser;
use clap_verbosity_flag::Verbosity;
use glob::Pattern;
use parse_size::{parse_size, Error};
use path_matchers::{glob, PathMatcher};
use std::fs::*;
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

use log::{info, warn};

use matching::get_path_matcher;

/// Command-line arguments
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Cli {
    /// Directory to rotate
    #[clap(value_parser)]
    directory: PathBuf,

    /// Maximum filesize of the directory. Supply a number in bytes or with a suffix, e.g. 3K, 5MiB, etc.
    #[clap(value_parser = size_parser)]
    max_size: u64,

    /// Dry-run (only print operations)
    #[clap(short, long, value_parser, default_value_t = false)]
    dryrun: bool,

    /// Consider files with the same stem as a group and only delete whole groups.
    #[clap(short, long, value_parser)]
    group: bool,

    /// A glob pattern to only consider a subset of files, both in the size estimation and deletion.
    #[clap(short, long, value_parser)]
    include_only: Option<String>,

    /// A glob pattern to exclude a subset of files, both in the size estimation and deletion.
    #[clap(short, long, value_parser, conflicts_with = "include-only")]
    exclude: Option<String>,

    /// A glob pattern to protect a subset of files from deletion
    #[clap(short, long, value_parser)]
    select_for_op: Option<String>,

    /// A glob pattern to protect a subset of files from deletion
    #[clap(short, long, value_parser, conflicts_with = "select-for-op")]
    protect_from_op: Option<String>,

    #[clap(flatten)]
    verbose: Verbosity,
}

fn size_parser(s: &str) -> Result<u64, Error> {
    parse_size(s)
}

fn file_filter<'a>(
    items: impl Iterator<Item = (DirEntry, Metadata)> + 'a,
    select_pattern: &'a Option<impl PathMatcher>,
    protect_pattern: &'a Option<impl PathMatcher>,
) -> impl Iterator<Item = (DirEntry, Metadata)> + 'a {
    // Returns files (not dirs) matching the optional pattern, including file metadata

    items.filter(move |x| {
        if let Some(p) = select_pattern {
            p.matches(&x.0.path().canonicalize().expect("Malformed Path")) && x.0.path().is_file()
        } else if let Some(p) = protect_pattern {
            !p.matches(&x.0.path().canonicalize().expect("Malformed Path")) && x.0.path().is_file()
        } else {
            x.0.path().is_file()
        }
    })
}

fn list_all_files(path: &Path) -> impl Iterator<Item = (DirEntry, Metadata)> {
    fn is_hidden(entry: &DirEntry) -> bool {
        entry
            .file_name()
            .to_str()
            .map(|s| s.starts_with("."))
            .unwrap_or(false)
    }
    WalkDir::new(path)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| !is_hidden(e))
        .filter_map(|x| match x {
            Ok(e) => {
                if e.path().is_file() {
                    Some(e)
                } else {
                    None
                }
            }
            Err(why) => {
                println!("Traversal Error: {}", why);
                None
            }
        })
        .map(|e| {
            (
                e.clone(),
                e.metadata().expect("Could not get metadata from file"),
            )
        })
}

fn register_operations(mut entries: Vec<(DirEntry, Metadata)>, size_to_free: u64) -> Vec<PathBuf> {
    // For now: Don't group, just blindly consume.
    // Assume entries to be sorted such that the ones to keep are first.
    // As a consequence, we consume from the end of the vector.
    let mut size_freed: u64 = 0;
    let mut operations: Vec<PathBuf> = Vec::new();
    while size_freed < size_to_free && entries.len() > 0 {
        if let Some(e) = entries.pop() {
            operations.push(e.0.into_path());
            size_freed += e.1.len();
        } else {
            // This is unreachable. When {if|while}-let chains are fully stabilized in 1.64
            // (https://github.com/rust-lang/rust/issues/53667), use a while-let chain
            unreachable!("Couldn't pop, but length is not zero!")
        }
    }
    return operations;
}

fn canonicalize_base_dir(path: &PathBuf) -> PathBuf {
    path.canonicalize()
        .expect("Directory path is not a proper path.")
}

fn main() {
    // Setup
    let settings = Cli::parse();
    env_logger::Builder::new()
        .filter_level(settings.verbose.log_level_filter())
        .init();

    // Parse settings
    let base_directory = canonicalize_base_dir(&settings.directory);
    // Log info
    info!("Culling directory: {}", base_directory.display());
    info!("Culling to size: {}", settings.max_size);

    if settings.group || settings.exclude.is_some() || settings.include_only.is_some() {
        warn!("Group-by is still not implemented")
    }

    // Canonicalize glob patterns
    let include_only_matcher = get_path_matcher(&base_directory, &settings.include_only);
    let exclude_matcher = get_path_matcher(&base_directory, &settings.exclude);
    let select_matcher = get_path_matcher(&base_directory, &settings.select_for_op);
    let protect_matcher = get_path_matcher(&base_directory, &settings.protect_from_op);

    // Get vec of all files
    let files: Vec<(DirEntry, Metadata)> = file_filter(
        list_all_files(&base_directory),
        &include_only_matcher,
        &exclude_matcher,
    )
    .collect();

    // Calculate size
    let current_size: u64 = files.iter().map(|f| f.1.len()).sum();
    let size_to_free = current_size.saturating_sub(settings.max_size);
    info!("Size to free: {}", size_to_free);
    // Possible early out
    if size_to_free == 0 {
        return ();
    }

    // Get vec of files available for operation (deletion)
    let mut deletable: Vec<(DirEntry, Metadata)> =
        file_filter(files.iter().cloned(), &select_matcher, &protect_matcher).collect();
    // Sort entries on last_modified
    deletable.sort_by_key(|x| {
        x.1.modified()
            .expect("Last Modified Time is not available on this platform")
    });

    for f in &deletable {
        info!("Matched file: {}", f.0.path().display())
    }

    // register_operations
    let operations = register_operations(deletable, size_to_free);
    // perform_operations

    if settings.dryrun {
        info!("Planned operations:");
        for op in operations {
            info!("Deleting file: {}", op.display())
        }
    }
}
