// Copyright 2026 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Embeds the git commit into the binary as `GWS_GIT_SHA` (empty if not built from git).

use std::path::Path;
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn main() {
    let sha = git(&["rev-parse", "--short=10", "HEAD"]).unwrap_or_default();
    println!("cargo:rustc-env=GWS_GIT_SHA={sha}");

    // Rebuild when HEAD moves (branch switch or new commit on current branch).
    if let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        let git_dir = Path::new(&git_dir);
        println!("cargo:rerun-if-changed={}", git_dir.join("HEAD").display());
        if let Some(common) = git(&["rev-parse", "--path-format=absolute", "--git-common-dir"]) {
            println!("cargo:rerun-if-changed={common}/packed-refs");
            if let Some(head_ref) = git(&["symbolic-ref", "-q", "HEAD"]) {
                println!("cargo:rerun-if-changed={common}/{head_ref}");
            }
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
}
