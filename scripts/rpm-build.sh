#!/usr/bin/env bash
# =============================================================================
# rpm-build.sh — Build an RPM for LinuxCamPAM (Fedora 44, Rust rewrite)
#
# Usage:
#   ./scripts/rpm-build.sh [--skip-models] [--skip-deps] [--skip-tests] [--clean] [--release <ver>]
#
# Options:
#   --skip-models   Skip downloading ONNX models (useful if already cached)
#   --skip-deps     Skip installing build dependencies (dnf install)
#   --skip-tests    Skip cargo test suite (also passes --nocheck to rpmbuild)
#   --clean         Clean BUILD, RPMS, and SRPMS directories before building
#   --release <n>   Set the RPM Release number (default: 1)
#   -h, --help      Show this help message
#
# The script:
#   1. Verifies Fedora release compatibility
#   2. Installs missing build dependencies via dnf (unless --skip-deps)
#   3. Downloads/verifies ONNX models with SHA-256 into rpm/SOURCES/ (unless --skip-models)
#   4. Runs release test suite to fail fast (unless --skip-tests)
#   5. Creates a source tarball from project tree
#   6. Calls rpmbuild to produce the final .rpm
#   7. Verifies and prints the built RPM package
#
# Output:  rpm/RPMS/<arch>/linuxcampam-*.rpm
# =============================================================================
set -euo pipefail

# ── Locate the project root (works when called from any CWD) ─────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# ── Defaults ─────────────────────────────────────────────────────────────────
SKIP_MODELS=false
SKIP_DEPS=false
SKIP_TESTS=false
CLEAN_BUILD=false
RPM_RELEASE=1

# ── Parse arguments ───────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
    case "$1" in
        --skip-models) SKIP_MODELS=true; shift ;;
        --skip-deps)   SKIP_DEPS=true;   shift ;;
        --skip-tests)  SKIP_TESTS=true;  shift ;;
        --clean)       CLEAN_BUILD=true; shift ;;
        --release)
            [[ $# -lt 2 ]] && { echo "Error: --release requires a value" >&2; exit 1; }
            RPM_RELEASE="$2"
            shift 2
            ;;
        -h|--help)
            sed -n '2,/^# ==/p' "$0" | sed 's/^# \?//' | head -n -1
            exit 0
            ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

# ── Derive version from Cargo.toml ────────────────────────────────────────────
CARGO_VERSION=$(grep '^version' "${PROJECT_ROOT}/Cargo.toml" \
    | head -1 | sed 's/.*"\(.*\)".*/\1/')
# Normalise "0.9.7+5" → "0.9.7.5" for RPM (RPM forbids '+' in Version field)
RPM_VERSION="${CARGO_VERSION//+/.}"

PKG_NAME="linuxcampam-${RPM_VERSION}"

# ── RPM build tree ────────────────────────────────────────────────────────────
RPM_DIR="${PROJECT_ROOT}/rpm"
SOURCES_DIR="${RPM_DIR}/SOURCES"
SPECS_DIR="${RPM_DIR}/SPECS"
BUILD_DIR="${RPM_DIR}/BUILD"
RPMS_DIR="${RPM_DIR}/RPMS"
SRPMS_DIR="${RPM_DIR}/SRPMS"

mkdir -p "${SOURCES_DIR}" "${BUILD_DIR}" "${RPMS_DIR}" "${SRPMS_DIR}"

if [[ "${CLEAN_BUILD}" == true ]]; then
    rm -rf "${BUILD_DIR:?}"/* "${RPMS_DIR:?}"/* "${SRPMS_DIR:?}"/*
fi

# ── Colour helpers ────────────────────────────────────────────────────────────
bold()  { printf '\033[1m%s\033[0m\n' "$*"; }
info()  { printf '\033[1;34m[INFO]\033[0m  %s\n' "$*"; }
ok()    { printf '\033[1;32m[ OK ]\033[0m  %s\n' "$*"; }
warn()  { printf '\033[1;33m[WARN]\033[0m  %s\n' "$*"; }
die()   { printf '\033[1;31m[ERR ]\033[0m  %s\n' "$*" >&2; exit 1; }

bold "═══════════════════════════════════════════════════"
bold " LinuxCamPAM RPM Builder — Fedora 44"
bold " Package : ${PKG_NAME}-${RPM_RELEASE}"
bold "═══════════════════════════════════════════════════"

# ── 1. Verify we're on Fedora 44 ─────────────────────────────────────────────
if [[ -f /etc/fedora-release ]]; then
    FED_VER=$(grep -oP '\d+' /etc/fedora-release | head -1 || echo "unknown")
    if [[ "${FED_VER}" != "44" ]]; then
        warn "This script targets Fedora 44 (detected: Fedora ${FED_VER})."
        warn "Continuing anyway — YMMV on other releases."
    else
        ok "Fedora 44 detected."
    fi
else
    die "/etc/fedora-release not found. This script requires Fedora."
fi

# ── 2. Check and install build dependencies ──────────────────────────────────
REQUIRED_PKGS=(
    rust
    cargo
    gcc
    pam-devel
    hidapi-devel
    libv4l-devel
    rpmdevtools
    rpm-build
    systemd-rpm-macros
    tar
    gzip
    sed
)

if [[ "${SKIP_DEPS}" == false ]]; then
    info "Checking build dependencies…"
    MISSING_PKGS=()
    for pkg in "${REQUIRED_PKGS[@]}"; do
        if ! rpm -q "${pkg}" &>/dev/null; then
            MISSING_PKGS+=("${pkg}")
        fi
    done

    # Ensure at least one download utility is present for model fetching
    if ! command -v curl &>/dev/null && ! command -v wget &>/dev/null; then
        MISSING_PKGS+=(curl)
    fi

    if [[ ${#MISSING_PKGS[@]} -eq 0 ]]; then
        ok "All build dependencies already satisfied."
    else
        info "Missing dependencies: ${MISSING_PKGS[*]}"
        info "Installing missing dependencies via dnf…"
        if [[ $EUID -eq 0 ]]; then
            dnf install -y "${MISSING_PKGS[@]}"
        elif command -v sudo &>/dev/null; then
            sudo dnf install -y "${MISSING_PKGS[@]}"
        else
            die "Cannot install missing packages without root or sudo: ${MISSING_PKGS[*]}"
        fi
        ok "Build dependencies installed."
    fi
else
    info "Skipping dependency install (--skip-deps)."
fi

# ── 3. Check Rust toolchain ───────────────────────────────────────────────────
if ! command -v rustc &>/dev/null; then
    die "rustc not found. Install Rust: https://rustup.rs or dnf install rust"
fi
RUST_VER=$(rustc --version | awk '{print $2}')
info "Rust toolchain: ${RUST_VER}"
if ! command -v cargo &>/dev/null; then
    die "cargo not found. Install Cargo: dnf install cargo"
fi

# ── 4. Download / verify ONNX models with SHA-256 ──────────────────────────────
YUNET_DEST="${SOURCES_DIR}/face_detection_yunet_2023mar.onnx"
SFACE_DEST="${SOURCES_DIR}/face_recognition_sface_2021dec.onnx"

YUNET_SHA256="8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4"
SFACE_SHA256="0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79"

verify_checksum() {
    local file="$1" expected_sha256="$2"
    if [[ ! -s "${file}" ]]; then
        return 1
    fi
    local actual_sha256
    actual_sha256=$(sha256sum "${file}" | awk '{print $1}')
    if [[ "${actual_sha256}" == "${expected_sha256}" ]]; then
        return 0
    fi
    return 1
}

download_file() {
    local url="$1" dest="$2" name="$3" expected_sha256="$4"
    if verify_checksum "${dest}" "${expected_sha256}"; then
        ok "${name}: already cached and verified ($(du -sh "${dest}" | cut -f1))."
        return 0
    elif [[ -e "${dest}" ]]; then
        warn "${name}: cached file failed checksum verification. Re-downloading…"
        rm -f "${dest}"
    fi

    info "Downloading ${name}…"
    local max_tries=3 delay=5
    for ((i=1; i<=max_tries; i++)); do
        local dl_ok=false
        if command -v curl &>/dev/null; then
            if curl -fSL --connect-timeout 15 --retry 2 -o "${dest}.tmp" "${url}"; then
                dl_ok=true
            fi
        elif command -v wget &>/dev/null; then
            if wget -q --show-progress -O "${dest}.tmp" "${url}"; then
                dl_ok=true
            fi
        else
            die "Neither curl nor wget available to download ${name}."
        fi

        if [[ "${dl_ok}" == true ]] && [[ -s "${dest}.tmp" ]]; then
            if verify_checksum "${dest}.tmp" "${expected_sha256}"; then
                mv "${dest}.tmp" "${dest}"
                ok "${name} downloaded and verified ($(du -sh "${dest}" | cut -f1))."
                return 0
            else
                warn "${name} download attempt ${i} produced invalid checksum."
            fi
        fi
        rm -f "${dest}.tmp"
        warn "Attempt ${i}/${max_tries} failed. Retrying in ${delay}s…"
        sleep "${delay}"
        delay=$((delay * 2))
    done
    die "Failed to download and verify ${name} after ${max_tries} attempts."
}

if [[ "${SKIP_MODELS}" == false ]]; then
    info "Checking and verifying ONNX models…"
    download_file \
        "https://github.com/opencv/opencv_zoo/raw/main/models/face_detection_yunet/face_detection_yunet_2023mar.onnx" \
        "${YUNET_DEST}" \
        "face_detection_yunet_2023mar.onnx" \
        "${YUNET_SHA256}"

    download_file \
        "https://huggingface.co/opencv/face_recognition_sface/resolve/main/face_recognition_sface_2021dec.onnx" \
        "${SFACE_DEST}" \
        "face_recognition_sface_2021dec.onnx" \
        "${SFACE_SHA256}"
else
    info "Skipping model download (--skip-models)."
    verify_checksum "${YUNET_DEST}" "${YUNET_SHA256}" || die "YuNet model invalid or missing: ${YUNET_DEST}. Re-run without --skip-models."
    verify_checksum "${SFACE_DEST}" "${SFACE_SHA256}" || die "SFace model invalid or missing: ${SFACE_DEST}. Re-run without --skip-models."
    ok "Models already present and verified in SOURCES."
fi

# Ensure project-local models directory is linked for direct cargo test runs
mkdir -p "${PROJECT_ROOT}/models"
ln -sfn "${YUNET_DEST}" "${PROJECT_ROOT}/models/face_detection_yunet_2023mar.onnx"
ln -sfn "${SFACE_DEST}" "${PROJECT_ROOT}/models/face_recognition_sface_2021dec.onnx"

# ── 5. Run tests (fail fast before wasting rpmbuild time) ────────────────────
if [[ "${SKIP_TESTS}" == false ]]; then
    info "Running release test suite…"
    cd "${PROJECT_ROOT}"
    cargo test --release --locked
    ok "All tests passed."
else
    info "Skipping test suite (--skip-tests)."
fi

# ── 6. Create source tarball ──────────────────────────────────────────────────
TARBALL="${SOURCES_DIR}/${PKG_NAME}.tar.gz"
info "Creating source tarball: ${PKG_NAME}.tar.gz"

cd "${PROJECT_ROOT}"

tar --exclude='.git' \
    --exclude='*/.git' \
    --exclude='./target' \
    --exclude='./rpm' \
    --exclude='./.idea' \
    --exclude='./models' \
    --exclude='./debian' \
    --exclude='*.swp' \
    --exclude='*~' \
    -czf "${TARBALL}" \
    --transform "s|^\.|${PKG_NAME}|" \
    -C "${PROJECT_ROOT}" \
    .

ok "Source tarball created ($(du -sh "${TARBALL}" | cut -f1))."

# ── 7. Run rpmbuild ───────────────────────────────────────────────────────────
info "Building RPM with rpmbuild…"
SYS_DIST=$(rpm --eval '%{?dist}' 2>/dev/null || echo "")
DIST_VAL="${SYS_DIST:-.fc44}"

RPMBUILD_ARGS=(
    --define "_topdir ${RPM_DIR}"
    --define "_sourcedir ${SOURCES_DIR}"
    --define "_builddir ${BUILD_DIR}"
    --define "_rpmdir ${RPMS_DIR}"
    --define "_srcrpmdir ${SRPMS_DIR}"
    --define "dist ${DIST_VAL}"
    --define "_release ${RPM_RELEASE}"
    --define "_version ${RPM_VERSION}"
)

if [[ "${SKIP_TESTS}" == true ]]; then
    RPMBUILD_ARGS+=(--nocheck)
fi

rpmbuild "${RPMBUILD_ARGS[@]}" -ba "${SPECS_DIR}/linuxcampam.spec"

# ── 8. Locate and report output ───────────────────────────────────────────────
BUILT_RPM=$(find "${RPMS_DIR}" -name "linuxcampam-${RPM_VERSION}-${RPM_RELEASE}*.rpm" ! -name "*.src.rpm" | head -1)
[[ -z "${BUILT_RPM}" ]] && BUILT_RPM=$(find "${RPMS_DIR}" -name "linuxcampam-*.rpm" ! -name "*.src.rpm" | sort -V | tail -1)
BUILT_SRPM=$(find "${SRPMS_DIR}" -name "linuxcampam-${RPM_VERSION}-${RPM_RELEASE}*.src.rpm" | head -1)
[[ -z "${BUILT_SRPM}" ]] && BUILT_SRPM=$(find "${SRPMS_DIR}" -name "linuxcampam-*.src.rpm" | sort -V | tail -1)

if [[ -z "${BUILT_RPM}" || ! -f "${BUILT_RPM}" ]]; then
    die "RPM build failed: binary RPM not found in ${RPMS_DIR}."
fi

bold ""
bold "═══════════════════════════════════════════════════"
bold " Build complete!"
bold "═══════════════════════════════════════════════════"
ok "Binary RPM : ${BUILT_RPM} ($(du -sh "${BUILT_RPM}" | cut -f1))"
[[ -n "${BUILT_SRPM}" ]] && ok "Source RPM : ${BUILT_SRPM} ($(du -sh "${BUILT_SRPM}" | cut -f1))"
bold ""
bold "Package details:"
rpm -qip "${BUILT_RPM}" | grep -E '^(Name|Version|Release|Architecture|Summary|Size)' || true
bold ""
bold "Package contents:"
rpm -qlp "${BUILT_RPM}"
bold ""
bold "To install:"
bold "  sudo dnf install '${BUILT_RPM}'"
bold ""
