#!/bin/sh
# SPDX-FileCopyrightText: 2026 OpenAI
#
# SPDX-License-Identifier: AGPL-3.0-or-later

# Build a local binary package from this checkout without modifying the source.
set -eu

for tool in dpkg-buildpackage dh cargo rustc; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "Missing $tool; see the Debian build prerequisites in README.md." >&2
        exit 1
    fi
done

repo=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
output="$repo/target/debian"
mkdir -p "$output"
work=$(mktemp -d "$output/build.XXXXXX")
trap 'rm -rf "$work"' EXIT
trap 'exit 1' HUP INT TERM

source_dir="$work/radpro2mqtt"
mkdir -p "$source_dir/packaging" "$source_dir/debian"
cp "$repo/Cargo.toml" "$repo/Cargo.lock" "$repo/README.md" "$repo/LICENSE" "$source_dir/"
cp -R "$repo/src" "$source_dir/"
cp "$repo/packaging/radpro2mqtt.service" "$repo/packaging/radpro2mqtt.env.example" "$source_dir/packaging/"
cp "$repo/packaging/debian/control" "$repo/packaging/debian/rules" "$source_dir/debian/"

version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$repo/Cargo.toml" | head -n 1)
if [ -z "$version" ]; then
    echo 'Cannot determine the crate version from Cargo.toml.' >&2
    exit 1
fi
if git -C "$repo" rev-parse --verify HEAD >/dev/null 2>&1; then
    version="$version+git$(git -C "$repo" rev-list --count HEAD).$(git -C "$repo" rev-parse --short HEAD)"
fi
cat > "$source_dir/debian/changelog" <<EOF
radpro2mqtt ($version-1) unstable; urgency=medium

  * Local build from the radpro2mqtt checkout.

 -- Stan Grams <sjg@haxx.space>  $(LC_ALL=C date -R)
EOF

cd "$source_dir"
dpkg-buildpackage --build=binary --no-sign
cp "$work"/*.deb "$work"/*.buildinfo "$work"/*.changes "$output/"
printf '\nPackages written to %s\n' "$output"
