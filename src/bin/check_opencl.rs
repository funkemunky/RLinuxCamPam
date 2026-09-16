use std::fs;
use std::path::Path;

fn main() {
    println!("=== OpenCL Diagnostic Tool ===");

    let mut opencl_available = false;
    let icd_dir = Path::new("/etc/OpenCL/vendors");
    if icd_dir.exists() {
        if let Ok(entries) = fs::read_dir(icd_dir) {
            let vendors: Vec<String> = entries
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "icd"))
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            if !vendors.is_empty() {
                opencl_available = true;
                println!("OpenCL ICD vendors found: {}", vendors.join(", "));
            }
        }
    }

    if Path::new("/usr/lib64/libOpenCL.so.1").exists() || Path::new("/usr/lib/libOpenCL.so.1").exists() {
        opencl_available = true;
        println!("OpenCL ICD Loader library is present.");
    }

    if opencl_available {
        println!("OpenCL is available on this system.");
        println!("Available OpenCL Platforms / Devices:");
        if let Ok(entries) = fs::read_dir("/sys/class/drm") {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with("card") && !name_str.contains('-') {
                    println!("  - DRM Device: {name_str}");
                }
            }
        }
    } else {
        println!("OpenCL is NOT available on this system.");
    }
}
