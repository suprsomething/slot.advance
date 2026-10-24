#!/bin/sh
# Syncthing for the RG SP (slot.advance 0.5.0, approved A3; pinned to 2.1.6, the version
# tested throughout Phase 0). Official linux-arm64 release from GitHub, checked against the
# SHA-256 published in Syncthing's signed sha256sum.txt.asc for that release.
#
#   sh scripts/syncthing.sh fetch DIR    download, check and unpack into DIR
#   sh scripts/syncthing.sh check FILE   check an unpacked program
set -eu

VERSION=2.1.6
NAME="syncthing-linux-arm64-v${VERSION}"
URL="https://github.com/syncthing/syncthing/releases/download/v${VERSION}/${NAME}.tar.gz"
SOURCE="https://github.com/syncthing/syncthing/releases/tag/v${VERSION}"
ARCHIVE_SHA256=f054de02462d5d58bd126908f2f92b63aba74f162a5be85e50e96d656e7db47d
PROGRAM_SHA256=68d16bb03d9568189cd39a79c32767bba75bd750c612968b3db4d9d6174c1d3a
LICENSE_SHA256=3f3d9e0024b1921b067d6f7f88deb4a60cbe7a78e76c64e3f1d7fc3b779b9d04

sha() { sha256sum "$1" 2>/dev/null | cut -d' ' -f1 || shasum -a 256 "$1" | cut -d' ' -f1; }

check() {
	got="$(sha "$1")"
	if [ "$got" != "$PROGRAM_SHA256" ]; then
		echo "syncthing: $1 is not the pinned Syncthing ${VERSION} program (sha256 $got)" >&2
		exit 1
	fi
}

case "${1:-}" in
	fetch)
		dir="${2:?usage: syncthing.sh fetch DIR}"
		if [ -f "$dir/syncthing" ] && [ "$(sha "$dir/syncthing")" = "$PROGRAM_SHA256" ]; then
			echo "syncthing ${VERSION} already in $dir"
			exit 0
		fi
		mkdir -p "$dir"
		tmp="$(mktemp -d)"
		trap 'rm -rf "$tmp"' EXIT
		curl -fsSL -o "$tmp/st.tar.gz" "$URL"
		got="$(sha "$tmp/st.tar.gz")"
		if [ "$got" != "$ARCHIVE_SHA256" ]; then
			echo "syncthing: download does not match the pinned sha256 (got $got)" >&2
			exit 1
		fi
		tar -xzf "$tmp/st.tar.gz" -C "$tmp"
		check "$tmp/$NAME/syncthing"
		if [ "$(sha "$tmp/$NAME/LICENSE.txt")" != "$LICENSE_SHA256" ]; then
			echo "syncthing: unexpected LICENSE.txt" >&2
			exit 1
		fi
		cp "$tmp/$NAME/syncthing" "$dir/syncthing"
		chmod 755 "$dir/syncthing"
		cp "$tmp/$NAME/LICENSE.txt" "$dir/LICENSE.txt"
		cp "$tmp/$NAME/AUTHORS.txt" "$dir/AUTHORS.txt"
		cat > "$dir/syncthing.txt" <<NOTE
Syncthing ${VERSION} (linux-arm64), unmodified, as released by the Syncthing project.
Licence: Mozilla Public License 2.0 (syncthing-LICENSE.txt). Authors: syncthing-AUTHORS.txt.
Source code: ${SOURCE}
Release archive: ${URL}
Archive sha256: ${ARCHIVE_SHA256}
Program sha256: ${PROGRAM_SHA256}
slot.advance runs it with automatic upgrades off; it is updated only with slot.advance releases.
NOTE
		echo "fetched syncthing ${VERSION} into $dir"
		;;
	check)
		check "${2:?usage: syncthing.sh check FILE}"
		echo "syncthing ${VERSION}: ok"
		;;
	*)
		echo "usage: syncthing.sh fetch DIR | check FILE" >&2
		exit 2
		;;
esac
