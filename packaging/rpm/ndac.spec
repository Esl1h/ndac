# Binary RPM built from the static musl release archive, for Fedora COPR.
# packaging.yml sets Version from each GitHub release and builds the SRPM.

%global debug_package %{nil}

Name:           ndac
Version:        0.0.0
Release:        1%{?dist}
Summary:        NextDNS-as-Code: manage NextDNS profiles from TOML files
License:        MIT OR Apache-2.0
URL:            https://github.com/Esl1h/ndac
Source0:        %{url}/releases/download/v%{version}/ndac-%{version}-x86_64-unknown-linux-musl.tar.gz
Source1:        %{url}/releases/download/v%{version}/ndac-%{version}-aarch64-unknown-linux-musl.tar.gz
ExclusiveArch:  x86_64 aarch64
BuildRequires:  systemd-rpm-macros

%description
ndac keeps the desired state of NextDNS profiles in TOML files, shows the
difference with the live state as a plan, and applies it.

%prep
%ifarch x86_64
%setup -q -T -b 0 -n ndac-%{version}-x86_64-unknown-linux-musl
%endif
%ifarch aarch64
%setup -q -T -b 1 -n ndac-%{version}-aarch64-unknown-linux-musl
%endif

%build

%install
install -Dpm0755 ndac %{buildroot}%{_bindir}/ndac
install -Dpm0644 completions/ndac.bash %{buildroot}%{bash_completions_dir}/ndac
install -Dpm0644 completions/_ndac %{buildroot}%{zsh_completions_dir}/_ndac
install -Dpm0644 completions/ndac.fish %{buildroot}%{fish_completions_dir}/ndac.fish
install -Dpm0644 man/ndac.1 %{buildroot}%{_mandir}/man1/ndac.1
install -Dpm0644 systemd/ndac-drift.service %{buildroot}%{_unitdir}/ndac-drift.service
install -Dpm0644 systemd/ndac-drift.timer %{buildroot}%{_unitdir}/ndac-drift.timer

%files
%license LICENSE-MIT LICENSE-APACHE
%doc README.md CHANGELOG.md
%{_bindir}/ndac
%{bash_completions_dir}/ndac
%{zsh_completions_dir}/_ndac
%{fish_completions_dir}/ndac.fish
%{_mandir}/man1/ndac.1*
%{_unitdir}/ndac-drift.service
%{_unitdir}/ndac-drift.timer

%changelog
* Wed Oct 21 2026 Esli Silva - 0.0.0-1
- Packaging template; packaging.yml sets the version for each release.
