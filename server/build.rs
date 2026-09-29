// 把应用图标与版本信息写进 exe 的 Windows 资源段，资源管理器里就能看到自己的 logo。
#[cfg(windows)]
fn main() {
    // 前端产物一变就重新编译，避免内嵌的界面过期。
    println!("cargo:rerun-if-changed=../dist");
    ensure_resource_compiler_on_path();
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("../assets/icon.ico");
    resource.set("ProductName", "Lythara Convert");
    resource.set("FileDescription", "Lythara Convert 本地媒体转换服务");
    resource.set("CompanyName", "LytharaLab");
    resource.set("LegalCopyright", "Copyright © 2026 LytharaLab");
    resource.set("OriginalFilename", "lythara-convert.exe");
    if let Err(err) = resource.compile() {
        println!("cargo:warning=图标资源编译失败（{err}），exe 将没有自定义图标");
    }
}

#[cfg(not(windows))]
fn main() {
    println!("cargo:rerun-if-changed=../dist");
}

/// 未激活 VS 开发环境时 PATH 里没有 rc.exe，这里按 SDK 布局自己找一下。
#[cfg(windows)]
fn ensure_resource_compiler_on_path() {
    use std::path::PathBuf;
    use std::process::Command;

    let usable = |dir: &PathBuf| dir.join("rc.exe").is_file();
    let mut found: Option<PathBuf> = None;

    if let Ok(paths) = std::env::var("PATH") {
        for entry in std::env::split_paths(&paths) {
            if usable(&entry) {
                found = Some(entry);
                break;
            }
        }
    }

    if found.is_none() {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(program_files) = std::env::var("ProgramFiles(x86)") {
            let kits = PathBuf::from(program_files).join("Windows Kits").join("10").join("bin");
            if let Ok(entries) = std::fs::read_dir(&kits) {
                let mut versions: Vec<PathBuf> = entries.filter_map(|entry| entry.ok().map(|item| item.path())).collect();
                versions.sort();
                versions.reverse();
                for version in versions {
                    candidates.push(version.join("x64"));
                }
            }
        }
        found = candidates.into_iter().find(usable);
    }

    if let Some(dir) = found {
        if let Ok(existing) = std::env::var("PATH") {
            let joined = std::env::join_paths(
                std::iter::once(dir.clone()).chain(std::env::split_paths(&existing)),
            );
            if let Ok(value) = joined {
                std::env::set_var("PATH", value);
            }
        }
        // 确认真的能跑起来，避免把坏路径塞进去。
        if Command::new("rc").arg("/?").output().is_err() {
            println!("cargo:warning=找到了 rc.exe 但无法执行：{}", dir.display());
        }
    } else {
        println!("cargo:warning=未找到 rc.exe（Windows SDK），跳过图标资源");
    }
}
