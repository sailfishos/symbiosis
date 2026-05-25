%global rustflags -Clink-arg=-Wl,-z,relro,-z,now

%if ! %{defined _cargo_offline}
%global _cargo_offline %{nil}
%endif

Name:           symbiosis
Version:        0.1.0
Release:        0
Summary:        Sailfish next generation TOHD

License:        BSD-3-Clause
URL:            https://github.com/sailfishos/symbiosis
Source0:        %{name}-%{version}.tar.bz2
Source1:        vendor.tar.zst
Source2:        cargo_config
Source101:      %{name}.service
Source102:      org.sailfishos.tohd1.service
Source103:      %{name}.conf

BuildRequires:  cargo >= 1.75.0
BuildRequires:  rust >= 1.75.0
BuildRequires:  rust-std-static >= 1.75.0

%define systemunitdir %{_prefix}/lib/systemd/system
%define dbussystemservicedir %{_datadir}/dbus/system-services
%define dbussystempolicydir %{_datadir}/dbus/system.d

%description
%{summary}.

%prep
%autosetup -a1 -n %{name}-%{version}

%if 0%{?_obs_build_project:1}
install -D -m 644 %{SOURCE2} .cargo/config
%endif


%build
# When cross-compiling under SB2 rust needs to know what arch to emit
# when nothing is specified on the command line. That usually defaults
# to "whatever rust was built as" but in SB2 rust is accelerated and
# would produce x86 and x86_64 so this is how it knows differently. Not needed
# for native x86 and x86_64 builds
%ifarch %arm
export SB2_RUST_TARGET_TRIPLE=armv7-unknown-linux-gnueabihf
%endif
%ifarch aarch64
export SB2_RUST_TARGET_TRIPLE=aarch64-unknown-linux-gnu
%endif
# This avoids a malloc hang in sb2 gated calls to execvp/dup2/chdir
# during fork/exec. It has no effect outside sb2 so doesn't hurt
# native builds.
%ifnarch %{ix86} x86_64
export SB2_RUST_EXECVP_SHIM="/usr/bin/env LD_PRELOAD=/usr/lib/libsb2/libsb2.so.1 /usr/bin/env"
export SB2_RUST_USE_REAL_EXECVP=Yes
export SB2_RUST_USE_REAL_FN=Yes
%endif

export RUSTFLAGS="%{rustflags}"
export CARGO_HOME=`pwd`/cargo-home/

export CARGO_OFFLINE="%{_cargo_offline}"

# Forcing cargo builds to use a single core in order to make it build more
# reliably. Let's revisit when we upgrade rust. JB#53588
%ifarch %arm aarch64
cargo build -j1 $CARGO_OFFLINE --locked --target $SB2_RUST_TARGET_TRIPLE --release
%else
cargo build -j1 $CARGO_OFFLINE --locked --release
%endif

%install
%define rustbuilddir target/release
%ifarch %arm
%define rustbuilddir target/armv7-unknown-linux-gnueabihf/release
%endif
%ifarch aarch64
%define rustbuilddir target/aarch64-unknown-linux-gnu/release
%endif

install -D -m0755 %{rustbuilddir}/%{name} %{buildroot}%{_bindir}/%{name}

# Systemd unit files and D-Bus configuration
install -D -m0644 %{SOURCE101} %{buildroot}%{systemunitdir}/%{name}.service
install -D -m0644 %{SOURCE102} %{buildroot}%{dbussystemservicedir}/org.sailfishos.tohd1.service
install -D -m0644 %{SOURCE103} %{buildroot}%{dbussystempolicydir}/%{name}.conf

%post
systemctl daemon-reload || :
if [ $1 == 1 ]
then
    systemctl enable --now %{name}.service || :
fi
systemctl reload-or-try-restart %{name}.service || :

%postun
if [ $1 == 0 ]
then
    systemctl disable --now %{name}.service || :
fi
systemctl daemon-reload || :

%files
%{_bindir}/%{name}
%{systemunitdir}/%{name}.service
%{dbussystemservicedir}/org.sailfishos.tohd1.service
%{dbussystempolicydir}/%{name}.conf
