%global crate_name linuxcampam
%global debug_package %{nil}

Name:           linuxcampam
Version:        %{?_version}%{!?_version:0.9.7.5}
Release:        %{?_release}%{!?_release:1}%{?dist}
Summary:        Face authentication PAM module for Linux

License:        MIT
URL:            https://github.com/Vladush/LinuxCamPAM
Source0:        https://github.com/Vladush/LinuxCamPAM/archive/v%{version}/%{crate_name}-%{version}.tar.gz
Source10:       https://github.com/opencv/opencv_zoo/raw/main/models/face_detection_yunet/face_detection_yunet_2023mar.onnx
Source11:       https://huggingface.co/opencv/face_recognition_sface/resolve/main/face_recognition_sface_2021dec.onnx

BuildRequires:  rust >= 1.75
BuildRequires:  cargo
BuildRequires:  gcc
BuildRequires:  pam-devel
BuildRequires:  hidapi-devel
BuildRequires:  libv4l-devel
BuildRequires:  systemd-rpm-macros

Requires:       pam
Requires:       hidapi
Requires:       v4l-utils
# OpenCL provider — any one satisfies (user installs for their GPU)
Suggests:       rocm-opencl
Suggests:       mesa-libOpenCL
Suggests:       intel-opencl

%{?systemd_requires}

%description
LinuxCamPAM provides face-unlock for Linux login, privilege escalation,
and lock screens (GDM, SDDM, LightDM) using AI models (YuNet/SFace via ONNX).
Hardware acceleration via OpenCL on Intel, AMD (ROCm/Rusticl), and NVIDIA.
Supports IR cameras, proximity sensors, and zero-interaction login.

This is the Rust rewrite; all C++ dependencies have been eliminated.

%prep
%autosetup -n %{crate_name}-%{version}
# Ensure models exist in the build root for tests and offline builds
mkdir -p models
cp -p %{SOURCE10} models/
cp -p %{SOURCE11} models/

%build
# Build all binaries and the PAM shared library in release mode.
# RUSTFLAGS: keep frame pointers for profiling; link pam dynamically.
export RUSTFLAGS="-C force-frame-pointers=yes"
cargo build --release --locked

%check
# Run the full test suite in release mode to reuse cached objects from %%build
export RUSTFLAGS="-C force-frame-pointers=yes"
cargo test --release --locked

%install
install -d %{buildroot}%{_bindir}
install -d %{buildroot}%{_libdir}/security
install -d %{buildroot}%{_sysconfdir}/linuxcampam/users
install -d %{buildroot}%{_unitdir}
install -d %{buildroot}%{_tmpfilesdir}
install -d %{buildroot}%{_datadir}/linuxcampam/models
install -d %{buildroot}%{_libexecdir}/linuxcampam

# Binaries
install -p -m 0755 target/release/linuxcampamd  %{buildroot}%{_bindir}/linuxcampamd
install -p -m 0755 target/release/linuxcampam   %{buildroot}%{_bindir}/linuxcampam
install -p -m 0755 target/release/check_opencl  %{buildroot}%{_libexecdir}/linuxcampam/check_opencl

# PAM module (must live in /usr/lib64/security for pam_module discovery)
install -p -m 0755 target/release/libpam_linuxcampam.so \
    %{buildroot}%{_libdir}/security/pam_linuxcampam.so

# Strip unneeded symbols to reduce package size
strip --strip-unneeded %{buildroot}%{_bindir}/linuxcampamd
strip --strip-unneeded %{buildroot}%{_bindir}/linuxcampam
strip --strip-unneeded %{buildroot}%{_libexecdir}/linuxcampam/check_opencl
strip --strip-unneeded %{buildroot}%{_libdir}/security/pam_linuxcampam.so

# Config file (noreplace so upgrades never clobber user edits)
install -p -m 0644 config/config.ini \
    %{buildroot}%{_sysconfdir}/linuxcampam/config.ini

# Systemd service
install -p -m 0644 scripts/linuxcampam.service \
    %{buildroot}%{_unitdir}/linuxcampam.service

# tmpfiles.d runtime directory entry for /run/linuxcampam
cat > %{buildroot}%{_tmpfilesdir}/linuxcampam.conf << 'EOF'
d /run/linuxcampam 0755 root root -
EOF

# OpenCL detection helper
install -p -m 0755 scripts/detect_opencl.sh \
    %{buildroot}%{_libexecdir}/linuxcampam/detect_opencl.sh

# ONNX models in standard datadir (/usr/share/linuxcampam/models)
install -p -m 0644 %{SOURCE10} \
    %{buildroot}%{_datadir}/linuxcampam/models/face_detection_yunet_2023mar.onnx
install -p -m 0644 %{SOURCE11} \
    %{buildroot}%{_datadir}/linuxcampam/models/face_recognition_sface_2021dec.onnx

# Ensure service ExecStart uses actual RPM macro paths
sed -i \
    -e "s|/usr/libexec/linuxcampam|%{_libexecdir}/linuxcampam|g" \
    -e "s|/usr/bin/linuxcampamd|%{_bindir}/linuxcampamd|g" \
    %{buildroot}%{_unitdir}/linuxcampam.service

%post
%systemd_post linuxcampam.service
%tmpfiles_create %{_tmpfilesdir}/linuxcampam.conf

echo ""
echo "LinuxCamPAM installed."
echo "  1. (Optional) Edit %{_sysconfdir}/linuxcampam/config.ini"
echo "  2. Enable & start the daemon:"
echo "       sudo systemctl enable --now linuxcampam.service"
echo "  3. Enroll your face:"
echo "       sudo linuxcampam add <username>"
echo "  4. Test authentication:"
echo "       linuxcampam test"
echo ""
echo "To enable PAM face auth system-wide, add to /etc/pam.d/system-auth:"
echo "  auth sufficient pam_linuxcampam.so"
echo ""

%preun
%systemd_preun linuxcampam.service

%postun
%systemd_postun_with_restart linuxcampam.service

# On full removal, optionally notify about retained user data
if [ $1 -eq 0 ]; then
    echo "LinuxCamPAM removed."
    echo "User enrollment data retained at %{_sysconfdir}/linuxcampam/users/"
    echo "Remove manually if desired: sudo rm -rf %{_sysconfdir}/linuxcampam"
fi

%files
%license upstream/LICENSE
%doc upstream/README.md upstream/CHANGELOG.md

# Binaries
%{_bindir}/linuxcampamd
%{_bindir}/linuxcampam
%{_libexecdir}/linuxcampam/

# PAM module
%{_libdir}/security/pam_linuxcampam.so

# Systemd unit and tmpfiles configuration
%{_unitdir}/linuxcampam.service
%{_tmpfilesdir}/linuxcampam.conf

# Config (noreplace = don't overwrite user edits on upgrade)
%config(noreplace) %{_sysconfdir}/linuxcampam/config.ini

# Model files
%{_datadir}/linuxcampam/

# Directories created by this package
%dir %{_sysconfdir}/linuxcampam
%dir %attr(700,root,root) %{_sysconfdir}/linuxcampam/users
%ghost %attr(0755,root,root) /run/linuxcampam

%changelog
* Wed Sep 16 2026 Dawson Hessler <dawsonhessler@example.com> - 0.9.7.5-1
- Initial Fedora 44 RPM package of the Rust rewrite
- Full parity with upstream LinuxCamPAM C++ codebase
- Real V4L2 frame capture (MJPEG, RGB24, BGR24, GREY, YUYV)
- Dynamic HIDAPI loading via dlopen
- PAM C ABI: pam_sm_authenticate, pam_sm_setcred, pam_sm_acct_mgmt
- 136 unit tests, 18 suites, 0 failures
