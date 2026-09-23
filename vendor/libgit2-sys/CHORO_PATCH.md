# Choro working-tree diff safety patch

Source: crates.io `libgit2-sys` 0.18.5+1.9.4 (bundled libgit2 1.9.4).
Upstream licenses are preserved in this directory and `libgit2/COPYING`.

`libgit2/src/libgit2/diff_file.c` uses the existing owned-buffer read path
for all regular working-tree files instead of memory-mapping unfiltered files.
Git objects and pack files retain their upstream behavior. Filters, binary
detection, size limits, symlinks, and diff algorithms remain unchanged.

This fixes the September 23, 2026 Choro SIGBUS in `xdl_hash_record`, reached
through `WorktreeChanges::capture` → `worktree_diffs` → `Patch::from_diff`.
The kernel reported `cluster_pagein past EOF`: a mutable working-tree mapping
was accessed after its backing file was shortened. A pre-read size check cannot
prevent that race. An owned read buffer either returns an ordinary read error
or remains valid independently of subsequent changes to the file.

The workspace enables `vendored-libgit2`, and this build script rejects
`LIBGIT2_NO_VENDOR` so a system library cannot silently bypass the patch.

Regression coverage lives in `crates/ide-core/src/git/diff.rs`: generate a patch,
truncate its source file, and then read the patch's original lines. Keep this
patch when upgrading until upstream provides equivalent protection.
