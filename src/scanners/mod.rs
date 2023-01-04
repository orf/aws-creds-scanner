mod ripgrep;

use crate::scanners::ripgrep::{run_full_check, run_quick_search, RipGrepMatch};
use crate::sources::PackageToProcess;
use anyhow::Result;
use itertools::Itertools;
use lazy_static::lazy_static;
use regex::Regex;
use std::fs::File;
use std::path::PathBuf;
use std::process::Command;
use std::{fs, io};
use temp_dir::TempDir;

// Two regular expressions to extract access keys from the matches.
lazy_static! {
    static ref ACCESS_KEY_REGEX: Regex =
        Regex::new("(('|\")(?:ASIA|AKIA|AROA|AIDA)([A-Z0-7]{16})('|\"))").unwrap();
    static ref SECRET_KEY_REGEX: Regex = Regex::new("(('|\")([a-zA-Z0-9+/]{40})('|\"))").unwrap();
}

#[derive(Debug, Clone)]
pub struct DownloadedPackage {
    pub package: PackageToProcess,
    _temp_dir: TempDir,
    extract_dir: PathBuf,
    download_path: PathBuf,
}

impl PartialEq for DownloadedPackage {
    fn eq(&self, other: &Self) -> bool {
        self.package == other.package
    }
}

#[derive(Debug)]
pub struct PossiblyMatchedPackage {
    pub downloaded_package: DownloadedPackage,
    pub matches: Vec<RipGrepMatch>,
}

#[derive(Debug, Clone)]
pub struct ScannerMatch {
    pub downloaded_package: DownloadedPackage,
    pub rg_match: RipGrepMatch,
    pub access_key: String,
    pub secret_key: String,
}

impl ScannerMatch {
    pub fn relative_path(&self) -> String {
        self.rg_match
            .path
            .to_str()
            .unwrap()
            .strip_prefix(self.downloaded_package.extract_dir.to_str().unwrap())
            .unwrap()
            .strip_prefix('/')
            .unwrap()
            .to_string()
    }
}

impl PartialEq for ScannerMatch {
    fn eq(&self, other: &Self) -> bool {
        self.access_key == other.access_key
            && self.secret_key == other.secret_key
            && self.downloaded_package == other.downloaded_package
    }
}

pub struct Scanner {}

impl Scanner {
    pub fn quick_check(
        &self,
        package: DownloadedPackage,
    ) -> Result<Option<PossiblyMatchedPackage>> {
        let matches = run_quick_search(&package.download_path)?;
        if !matches.is_empty() {
            Ok(Some(PossiblyMatchedPackage {
                downloaded_package: package,
                matches,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn full_check(&self, package: PossiblyMatchedPackage) -> Result<Vec<ScannerMatch>> {
        extract_package(&package.downloaded_package)?;
        let matches = run_full_check(&package.downloaded_package.extract_dir)?;
        println!("matches: {:?}", matches);
        let mut matched_keys = vec![];
        // The output may contain multiple matches for our second-stage regex.
        // Here we create a cartesian product product of all matches.
        for rg_match in &matches {
            let matches = ACCESS_KEY_REGEX
                .find_iter(&rg_match.lines)
                .cartesian_product(
                    SECRET_KEY_REGEX
                        .find_iter(&rg_match.lines)
                        .collect::<Vec<_>>(),
                )
                .map(|(key, secret)| (trim_quotes(key.as_str()), trim_quotes(secret.as_str())));

            matched_keys.extend(
                matches
                    .into_iter()
                    .map(|(access_key, secret_key)| ScannerMatch {
                        downloaded_package: package.downloaded_package.clone(),
                        rg_match: rg_match.clone(),
                        access_key,
                        secret_key,
                    }),
            )
        }

        Ok(matched_keys)
    }

    pub fn download_package(&self, package: &PackageToProcess) -> Result<DownloadedPackage> {
        let temp_dir = TempDir::new()?;
        let temp_dir_path = temp_dir.path();
        let download_dir = temp_dir_path.join("download");
        let extract_dir = temp_dir_path.join("extracted");
        fs::create_dir_all(&extract_dir)?;
        fs::create_dir_all(&download_dir)?;

        let download_path = download_dir.join(package.file_name());

        let mut out = File::create(&download_path)?;
        let mut resp =
            reqwest::blocking::get(package.download_url.to_string())?.error_for_status()?;
        io::copy(&mut resp, &mut out)?;
        Ok(DownloadedPackage {
            package: package.clone(),
            _temp_dir: temp_dir,
            extract_dir,
            download_path,
        })
    }
}

fn extract_package(package: &DownloadedPackage) -> Result<()> {
    Command::new("./scripts/extract-fs.sh")
        .args([
            package.extract_dir.to_str().unwrap(),
            package.download_path.to_str().unwrap(),
        ])
        .output()?;
    Ok(())
}

fn trim_quotes(string: &str) -> String {
    string[1..string.len() - 1].to_string()
}
