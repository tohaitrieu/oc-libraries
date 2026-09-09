#!/usr/bin/env bash
# Build FFmpeg for phones, LGPL, DYNAMIC.
#
#   ./build-mobile.sh ios-sim      → simulator on an Apple-silicon Mac
#   ./build-mobile.sh ios          → iPhone
#   ./build-mobile.sh android      → arm64 device (needs ANDROID_NDK_ROOT)
#
# Why this file lives in the open repo: it is the licence boundary. LGPL asks that whoever holds a
# copy can replace this library with their own build — so the recipe has to be public, and the link
# has to stay DYNAMIC. Static linking would drag the obligation into the product's own object files.
#
# No --enable-gpl, and no x264/x265: those are what makes an FFmpeg build GPL. H.264 comes from the
# platform's own hardware encoder, which a stock LGPL build reaches like any other codec.
set -euo pipefail
cd "$(dirname "$0")"

# Pinned. The Rust binding (`ffmpeg-next` 9) tracks one major line, and a mismatch shows up as
# missing symbols at link time rather than as a version complaint.
TAG="n9.0.1"
SRC="$PWD/build/ffmpeg-$TAG"
OUT_ROOT="$PWD/prefix"

WHAT="${1:-}"
[ -n "$WHAT" ] || { echo "cần: ios | ios-sim | android" >&2; exit 2; }

if [ ! -d "$SRC" ]; then
  mkdir -p "$PWD/build"
  echo "→ tải FFmpeg $TAG"
  git clone --depth 1 --branch "$TAG" https://github.com/FFmpeg/FFmpeg.git "$SRC"
fi

# What the renderer actually needs: read mp4/mov/mp3/wav and stills, write mp4 with the platform's
# H.264 encoder. Everything else is off, which is what keeps the library small enough to embed and
# short enough to audit.
COMMON=(
  --disable-everything --disable-programs --disable-doc --disable-debug
  --enable-shared --disable-static
  --disable-autodetect
  --enable-swscale --enable-swresample
  --enable-decoder=h264,hevc,mpeg4,aac,mp3,pcm_s16le,pcm_f32le,mjpeg,png
  --enable-encoder=aac,mjpeg,png
  --enable-demuxer=mov,mp3,wav,aac,image2,matroska
  --enable-muxer=mp4,mov,mp3,wav,image2
  --enable-parser=h264,hevc,aac,mpeg4video,mjpeg,png
  --enable-protocol=file
  --enable-filter=scale,aresample,format,null,anull
  # Bitstream filters, and NOT an optional extra: `--disable-everything` turns these off too, and
  # then the platform encoder opens with "Bitstream filter not found" — an error that names neither
  # the filter nor the build flag that removed it. MediaCodec needs the annex-b conversion to put
  # h264 into mp4 at all, and `extract_extradata` to hand the muxer the parameter sets.
  --enable-bsf=h264_mp4toannexb,hevc_mp4toannexb,extract_extradata,aac_adtstoasc,null
)

case "$WHAT" in
  ios|ios-sim)
    if [ "$WHAT" = ios ]; then
      SDK=iphoneos; MIN="-mios-version-min=16.4"
    else
      SDK=iphonesimulator; MIN="-mios-simulator-version-min=16.4"
    fi
    SYSROOT="$(xcrun --sdk $SDK --show-sdk-path)"
    OUT="$OUT_ROOT/$WHAT"
    cd "$SRC"
    make distclean >/dev/null 2>&1 || true
    ./configure "${COMMON[@]}" \
      --prefix="$OUT" \
      --enable-cross-compile --target-os=darwin --arch=arm64 \
      --cc="xcrun -sdk $SDK clang" \
      --extra-cflags="-arch arm64 -isysroot $SYSROOT $MIN -fembed-bitcode-marker" \
      --extra-ldflags="-arch arm64 -isysroot $SYSROOT $MIN" \
      --enable-videotoolbox --enable-encoder=h264_videotoolbox --enable-hwaccel=h264_videotoolbox
    ;;
  android)
    : "${ANDROID_NDK_ROOT:?đặt ANDROID_NDK_ROOT trỏ tới NDK}"
    TOOLS="$ANDROID_NDK_ROOT/toolchains/llvm/prebuilt/darwin-x86_64/bin"
    OUT="$OUT_ROOT/android-arm64"
    cd "$SRC"
    make distclean >/dev/null 2>&1 || true
    ./configure "${COMMON[@]}" \
      --prefix="$OUT" \
      --enable-cross-compile --target-os=android --arch=aarch64 \
      --cross-prefix="$TOOLS/llvm-" \
      --cc="$TOOLS/aarch64-linux-android24-clang" \
      --sysroot="$ANDROID_NDK_ROOT/toolchains/llvm/prebuilt/darwin-x86_64/sysroot" \
      --enable-jni --enable-mediacodec --enable-encoder=h264_mediacodec \
      --enable-decoder=h264_mediacodec,hevc_mediacodec
    ;;
  *) echo "cần: ios | ios-sim | android" >&2; exit 2;;
esac

make -j"$(sysctl -n hw.ncpu)"
make install
echo "→ $OUT"
ls "$OUT/lib"
