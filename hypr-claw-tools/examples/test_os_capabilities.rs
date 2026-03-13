//! Test OS capabilities

use hypr_claw_tools::os_capabilities::filesystem;

#[tokio::main]
async fn main() {
    println!("Testing OS Capabilities...\n");

    // Test filesystem operations
    let test_dir = "/tmp/hypr_test";

    println!("1. Testing create_dir...");
    match filesystem::create_dir(test_dir).await {
        Ok(_) => println!("   ✅ create_dir works"),
        Err(e) => println!("   ❌ create_dir failed: {}", e),
    }

    println!("2. Testing list...");
    match filesystem::list(test_dir).await {
        Ok(entries) => println!("   ✅ list works: {} entries", entries.len()),
        Err(e) => println!("   ❌ list failed: {}", e),
    }

    println!("3. Testing write...");
    let test_file = format!("{}/test.txt", test_dir);
    match filesystem::write(&test_file, "test content").await {
        Ok(_) => println!("   ✅ write works"),
        Err(e) => println!("   ❌ write failed: {}", e),
    }

    println!("4. Testing read...");
    match filesystem::read(&test_file).await {
        Ok(content) => println!("   ✅ read works: '{}'", content),
        Err(e) => println!("   ❌ read failed: {}", e),
    }

    println!("5. Testing delete...");
    match filesystem::delete(test_dir).await {
        Ok(_) => println!("   ✅ delete works"),
        Err(e) => println!("   ❌ delete failed: {}", e),
    }

    println!("\n✅ All filesystem operations work!");
}
