fn main() {
    tauri_build::build();
    // tauri-codegen 把 icons/icon.icns 编译期嵌入二进制（dev 模式 Dock 图标，
    // 见 generate_context! 的 app_icon），但它不监听图标文件变更：
    // 只改图标不重编会一直用旧的嵌入字节，这里显式声明依赖。
    println!("cargo:rerun-if-changed=icons");
}
