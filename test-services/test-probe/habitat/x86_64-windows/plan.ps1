$pkg_name = "test-probe"
$pkg_origin = "habitat-testing"
$pkg_version = "0.1.0"
$pkg_maintainer = "The Habitat Maintainers <humans@habitat.sh>"
$pkg_license = @("Apache-2.0")
$pkg_bin_dirs = @("bin")
$pkg_build_deps = @(
    "core/rust"
)

$pkg_binds_optional = @{
    thing_with_a_port="port"
}

function Invoke-Prepare {
    $env:CARGO_TARGET_DIR = "$HAB_CACHE_SRC_PATH/$pkg_dirname"
    Write-BuildLine "Setting env:CARGO_TARGET_DIR=$env:CARGO_TARGET_DIR"
}

function Invoke-Build {
    Push-Location "$PLAN_CONTEXT"
    try {
        cargo build --verbose
        if($LASTEXITCODE -ne 0) {
            Write-Error "Cargo build failed!"
        }
    } finally { Pop-Location }
}

function Invoke-Install {
    Copy-Item "$env:CARGO_TARGET_DIR/debug/test-probe.exe" "$pkg_prefix/bin/test-probe.exe"
}
