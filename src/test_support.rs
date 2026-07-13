//! Shared Ruby version fixtures for tests.

/// Versions that satisfy `is_regular_release`.
///
/// Ruby 3.x only supports minors `>= 2` (3.0/3.1 are end-of-life); Ruby 4.x
/// supports minors `>= 0`.
pub const GOOD_VERSIONS: &[&str] = &[
    "3.3.0", "3.3.12", "3.2.0", "3.2.11", "3.2.2", "4.0.0", "4.0.5", "4.1.0", "4.2.7",
];

/// Versions that fail `is_regular_release`.
pub const BAD_VERSIONS: &[&str] = &[
    "2.7.0",
    "3.2.0-preview1",
    "3.2.0-rc2",
    "3.1.5-something",
    "3.1.0",
    "3.1.12",
    "3.0.5",
    "3.0.16",
    "4.0.0-preview1",
    "4.1.0-rc1",
    "5.0.0",
];
