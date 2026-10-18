#!/usr/bin/env bash
# Build small disk images in several macOS filesystems, mount each, run the probe on the mount point
# and detach. Exploratory (work package R03): a failure to create one image is reported, not fatal.
set -uo pipefail

probe=${1:?usage: images.sh /path/to/probe}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

run_image() {
    local fs=$1 tag=$2
    local img="$work/$tag.dmg" mnt="$work/mnt-$tag"
    echo "::group::hdiutil $fs"
    mkdir -p "$mnt"
    # hdiutil can report "Resource busy" right after a detach; one retry is enough.
    if ! hdiutil create -size 100m -fs "$fs" -volname "OMG$tag" "$img" \
        && ! { sleep 3; hdiutil create -size 100m -fs "$fs" -volname "OMG$tag" "$img"; }; then
        echo "image creation failed for '$fs'"
        echo "::endgroup::"
        return
    fi
    if ! hdiutil attach -nobrowse -mountpoint "$mnt" "$img"; then
        echo "attach failed for '$fs'"
        echo "::endgroup::"
        return
    fi
    mount | grep -F "$mnt" || true
    "$probe" "$mnt" | sed -n '/^\[filesystem\] .*mnt-/,$p'
    # Case sensitivity is a filesystem property the scanner must not assume.
    touch "$mnt/Case" "$mnt/case" 2>/dev/null
    echo "entries after creating Case and case: $(ls -1 "$mnt" | grep -ci '^case$')"
    hdiutil detach "$mnt" >/dev/null || hdiutil detach -force "$mnt" >/dev/null || true
    echo "::endgroup::"
}

run_image "APFS" "apfs"
run_image "Case-sensitive APFS" "csapfs"
run_image "HFS+" "hfs"
run_image "Case-sensitive HFS+" "cshfs"
run_image "ExFAT" "exfat"
run_image "MS-DOS FAT32" "fat32"
