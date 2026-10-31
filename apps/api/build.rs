// sqlx::migrate! embeds the migrations at compile time, and Cargo would not
// rebuild for a migration file that was only added.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
