# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.2] - Unreleased

### Added

- SSL support #10

### Fixed

- AT+CFUN timeout set to 1 sec but it must be 15 acording to Quectel manual #11

## [0.4.1] - 2025-11-12

### Fixed

- Fixed bug when parsing NTP and NITZ responses (https://gitlab.com/scrobotics/embedded-rs/quectel-atat-rs/-/issues/8).

## [0.4.0] - 2025-09-15

### Added

- Support to configure any RAT order and any frequency band at run time.
- Features `bg95` and `bg96` (`bg95` set as default).

### Removed

- Removed features (`bands_usa` and `bands_eu`) to configure frequency bands at build time.

### Fixed

- If no APN user/pass provided, auth method set to None.

## [0.3.0] - 2025-08-27

### Added

- Support for GNSS requests.
- Configure USA frequency bands (`bands_usa` feature flag).

## [0.2.0] - 2024-09-20

### Changed

- Time retrieved from NTP instead of NITZ.

## [0.1.X]

Initial functional release.
