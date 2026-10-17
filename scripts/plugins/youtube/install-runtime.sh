#!/bin/sh
set -eu

architecture=${1:?Missing Docker target architecture}
destination=${2:?Missing installation destination}
case "$architecture" in
    amd64)
        ytdlp_asset=yt-dlp_linux.zip
        ytdlp_sha256=32e72032766bef9199d99d15beb69fd52e46df8f8b06f0d8745db59e04d339e9
        deno_asset=deno-x86_64-unknown-linux-gnu.zip
        deno_sha256=c6527f24f4b16031d3ae4fa9f658d5f11534c8d84ce7dc8502420280919c3490
        ;;
    arm64)
        ytdlp_asset=yt-dlp_linux_aarch64.zip
        ytdlp_sha256=4e27ad43f3a34bacffd078694eb3edbb4e3b378e7da44edab2be02e98555516e
        deno_asset=deno-aarch64-unknown-linux-gnu.zip
        deno_sha256=c832298b1ad4422481334855f6003e0f54145762c5a134f20a489511d2f65bbf
        ;;
    *)
        printf '%s\n' 'Unsupported YouTube plugin target architecture' >&2
        exit 1
        ;;
esac

download() {
    curl --fail --silent --show-error --location \
        --proto '=https' --proto-redir '=https' --tlsv1.2 \
        --connect-timeout 15 --max-time 180 --max-filesize 67108864 \
        --retry 2 --retry-delay 2 --retry-max-time 360 \
        --output "$2" "$1"
    printf '%s  %s\n' "$3" "$2" | sha256sum --check --strict --status
}

staging=$(mktemp -d)
trap 'rm -rf "$staging"' EXIT HUP INT TERM
download "https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/$ytdlp_asset" \
    "$staging/yt-dlp.zip" "$ytdlp_sha256"
download "https://github.com/denoland/deno/releases/download/v2.9.7/$deno_asset" \
    "$staging/deno.zip" "$deno_sha256"
download "https://github.com/denoland/deno/releases/download/v2.9.7/deno_src.tar.gz" \
    "$staging/deno-source.tar.gz" 21069d2f4dd65b6832e3f5c373c24a43a8d35cb3d68d3841e15d0582bed39ea8

mkdir -p "$destination/opt/yt-dlp" "$destination/usr/local/bin" \
    "$destination/usr/local/share/licenses/deno" "$staging/deno"
unzip -q "$staging/yt-dlp.zip" -d "$destination/opt/yt-dlp"
test -f "$destination/opt/yt-dlp/${ytdlp_asset%.zip}"
test -d "$destination/opt/yt-dlp/_internal"
test -s "$destination/opt/yt-dlp/_internal/THIRD_PARTY_LICENSES.txt"
mv "$destination/opt/yt-dlp/${ytdlp_asset%.zip}" "$destination/opt/yt-dlp/yt-dlp"
unzip -q "$staging/deno.zip" -d "$staging/deno"
test -f "$staging/deno/deno"
mv "$staging/deno/deno" "$destination/usr/local/bin/deno"
cp -a "$staging/deno/." "$destination/usr/local/share/licenses/deno/"
tar -tzf "$staging/deno-source.tar.gz" > "$staging/deno-source-files"
while IFS= read -r entry; do
    case "${entry##*/}" in
        *LICENSE*|*LICENCE*|*COPYING*|*NOTICE*) printf '%s\n' "$entry" ;;
    esac
done < "$staging/deno-source-files" > "$staging/deno-notice-files"
test -s "$staging/deno-notice-files"
mkdir -p "$destination/usr/local/share/licenses/deno/source-notices"
tar -xzf "$staging/deno-source.tar.gz" --no-same-owner --no-same-permissions \
    -C "$destination/usr/local/share/licenses/deno/source-notices" \
    --no-recursion --verbatim-files-from --files-from "$staging/deno-notice-files"
license=$(awk -F/ '($0 == "LICENSE.md") || (NF == 2 && $2 == "LICENSE.md") { print; count++ } END { if (count != 1) exit 1 }' "$staging/deno-notice-files")
tar -xOzf "$staging/deno-source.tar.gz" -- "$license" \
    > "$destination/usr/local/share/licenses/deno/LICENSE.md"
test -s "$destination/usr/local/share/licenses/deno/LICENSE.md"
chown -R 0:0 "$destination"
chmod -R a+rX,a-w "$destination"
chmod 0555 "$destination/opt/yt-dlp/yt-dlp" "$destination/usr/local/bin/deno"
