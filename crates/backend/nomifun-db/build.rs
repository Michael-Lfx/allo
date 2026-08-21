// sqlx::migrate!() embeds the migrations directory at macro-expansion time,
// but cargo only tracks files it is told about. Without this declaration,
// adding or editing a migration file does not trigger a rebuild of this
// crate, so running dev binaries keep the previous migration set and drift
// from the schema the code expects (e.g. "no such column" at runtime).
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
