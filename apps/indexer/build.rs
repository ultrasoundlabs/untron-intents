fn main() {
    // `sqlx::migrate!()` embeds migrations at compile time, but adding a new file in
    // `db/migrations/` won't automatically trigger a rebuild unless we tell Cargo to
    // watch that path.
    println!("cargo:rerun-if-changed=db/migrations");
}
