// Embed native resources in the Windows application binaries.

fn main() {
    println!("cargo:rerun-if-changed=windows/tank.rc");
    println!("cargo:rerun-if-changed=assets/icons/tank.ico");
    println!("cargo:rerun-if-changed=screensaver/windows/matrix/settings.rc");
    println!("cargo:rerun-if-changed=screensaver/windows/matrix/matrix.manifest");

    if std::env::var_os("MATRIX_SKIP_WINDOWS_RESOURCES").is_some() {
        return;
    }

    embed_resource::compile_for("windows/tank.rc", ["tank"], embed_resource::NONE)
        .manifest_optional()
        .expect("failed to compile Tank resources");

    embed_resource::compile_for(
        "screensaver/windows/matrix/settings.rc",
        ["matrix-saver"],
        embed_resource::NONE,
    )
    .manifest_required()
    .expect("failed to compile Windows screen saver resources");
}
