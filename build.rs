use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use bindgen::{Bindings, Builder};
use cc::Build;
use glob::glob;
use serde::Deserialize;

const CONFIG_FILE: &str = "arduino.yaml";

#[derive(Debug, Deserialize)]
struct BindgenLists {
    pub allowlist_function: Vec<String>,
    pub allowlist_type: Vec<String>,
    pub blocklist_function: Vec<String>,
    pub blocklist_type: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Config {
    pub arduino_home: String,
    pub external_libraries_home: String,
    pub core_version: String,
    pub variant: String,
    pub avr_gcc_version: String,
    pub arduino_libraries: Vec<String>,
    pub external_libraries: Vec<String>,
    pub definitions: HashMap<String, String>,
    pub flags: Vec<String>,
    pub bindgen_lists: BindgenLists,
    pub bindings_file: String,
}

impl Config {
    fn arduino_package_path(&self) -> PathBuf {
        let expanded = envmnt::expand(&self.arduino_home, None);
        let arduino_home_path = PathBuf::from(expanded);
        arduino_home_path.join("packages").join("arduino")
    }

    fn core_path(&self) -> PathBuf {
        self.arduino_package_path()
            .join("hardware")
            .join("avr")
            .join(&self.core_version)
    }

    fn avr_gcc_home(&self) -> PathBuf {
        self.arduino_package_path()
            .join("tools")
            .join("avr-gcc")
            .join(&self.avr_gcc_version)
    }

    fn avg_gcc(&self) -> PathBuf {
        self.avr_gcc_home().join("bin").join("avr-gcc")
    }

    fn arduino_core_path(&self) -> PathBuf {
        self.core_path().join("cores").join("arduino")
    }

    fn arduino_include_dirs(&self) -> Vec<PathBuf> {
        let variant_path = self.core_path().join("variants").join(&self.variant);
        let avr_gcc_include_path = self.avr_gcc_home().join("avr").join("include");
        vec![self.arduino_core_path(), variant_path, avr_gcc_include_path]
    }

    fn arduino_libraries_path(&self) -> Vec<PathBuf> {
        let library_root = self.core_path().join("libraries");
        let mut result = vec![];
        for library in &self.arduino_libraries {
            result.push(library_root.join(library).join("src"))
        }
        result
    }

    fn external_libraries_path(&self) -> Vec<PathBuf> {
        let expanded = envmnt::expand(&self.external_libraries_home, None);
        let external_library_root = PathBuf::from(expanded);
        let mut result = vec![];
        for library in &self.external_libraries {
            result.push(external_library_root.join(library).join("src"))
        }
        result
    }

    fn include_dirs(&self) -> Vec<PathBuf> {
        let mut result = self.arduino_include_dirs();
        result.extend(self.arduino_libraries_path());
        result.extend(self.external_libraries_path());
        result
    }

    fn project_files(&self, patten: &str) -> Vec<PathBuf> {
        let mut result =
            files_in_folder(self.arduino_core_path().to_string_lossy().as_ref(), patten);
        let mut libraries = self.arduino_libraries_path();
        libraries.extend(self.external_libraries_path());

        let pattern = format!("**/{}", patten);
        for library in libraries {
            let lib_sources = files_in_folder(library.to_string_lossy().as_ref(), &pattern);
            result.extend(lib_sources);
        }

        result
    }

    fn cpp_files(&self) -> Vec<PathBuf> {
        self.project_files("*.cpp")
    }

    fn c_files(&self) -> Vec<PathBuf> {
        self.project_files("*.c")
    }

    fn bindgen_headers(&self) -> Vec<PathBuf> {
        let mut result = vec![];
        for library in self.external_libraries_path() {
            let lib_headers = files_in_folder(library.to_string_lossy().as_ref(), "*.h");
            println!("Loading files {:?} from folder {:?}", lib_headers, library.to_string_lossy());
            result.extend(lib_headers);
        }
        result
    }
}

fn files_in_folder(folder: &str, pattern: &str) -> Vec<PathBuf> {
    let cpp_pattern = format!("{}/{}", folder, pattern);
    let mut results = vec![];
    for cpp_file in glob(&cpp_pattern).unwrap() {
        let file = cpp_file.unwrap();
        if !file.ends_with("main.cpp") {
            results.push(file);
        }
    }
    results
}

pub fn add_source_file(builder: &mut Build, files: Vec<PathBuf>) {
    for file in files {
        println!("cargo:rerun-if-changed={}", file.to_string_lossy());
        builder.file(file);
    }
}

fn configure_arduino(config: &Config) -> Build {
    let mut builder = Build::new();
    for (k, v) in &config.definitions {
        builder.define(k, v.as_str());
    }
    for flag in &config.flags {
        builder.flag(flag);
    }
    builder
        .compiler(config.avg_gcc())
        .flag("-Os")
        .cpp_set_stdlib(None)
        .flag("-fno-exceptions")
        .flag("-ffunction-sections")
        .flag("-fdata-sections");

    for include_dir in config.include_dirs() {
        builder.include(include_dir);
    }
    builder
}

fn compile_arduino(config: &Config) {
    let mut builder = configure_arduino(&config);
    builder.cpp(true)
        .flag("-std=gnu++11")
        .flag("-fpermissive")
        .flag("-fno-threadsafe-statics");
    add_source_file(&mut builder, config.cpp_files());

    builder.compile("libarduino_c++.a");
    println!("cargo:rustc-link-lib=static=arduino_c++");

    let mut builder = configure_arduino(&config);
    builder.flag("-std=gnu11");
    add_source_file(&mut builder, config.c_files());
    builder.compile("libarduino_c.a");
    println!("cargo:rustc-link-lib=static=arduino_c");
}

fn configure_arduino_bindgen(config: &Config) -> Builder {
    let mut builder = Builder::default();
    for (k, v) in &config.definitions {
        builder = builder.clang_arg(&format!("-D{}={}", k, v));
    }
    for flag in &config.flags {
        builder = builder.clang_arg(flag);
    }

    builder = builder
        .clang_args(&["-x", "c++", "-std=gnu++11"])
        .use_core()
        .size_t_is_usize(true)
        .layout_tests(false)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));

    for item in &config.bindgen_lists.allowlist_function {
        builder = builder.allowlist_function(item);
    }
    for item in &config.bindgen_lists.allowlist_type {
        builder = builder.allowlist_type(item);
    }
    for item in &config.bindgen_lists.blocklist_function {
        builder = builder.blocklist_function(item);
    }
    for item in &config.bindgen_lists.blocklist_type {
        builder = builder.blocklist_type(item);
    }

    for include_dir in config.include_dirs() {
        builder = builder.clang_arg(&format!("-I{}", include_dir.to_string_lossy()));
    }
    for header in config.bindgen_headers() {
        builder = builder.header(header.to_string_lossy());
    }

    return builder;
}

fn generate_bindings(config: &Config) {
    let generated_bindings = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(&config.bindings_file);

    if config.external_libraries.is_empty() {
        if generated_bindings.exists() {
            fs::remove_file(generated_bindings)
                .unwrap_or_else(|e| panic!("Bindings file {} exists, however could not delete it: {}", config.bindings_file, e));
        }

        return;
    }

    let builder = configure_arduino_bindgen(&config);

    let bindings: Bindings = builder.generate()
        .unwrap_or_else(|e| panic!("Unable to generate bindings: {}", e));
    return bindings.write_to_file(generated_bindings)
        .unwrap_or_else(|e| panic!("Could not write generated bindings to {}: {}", config.bindings_file, e));
}

fn main() {
    println!("cargo:rerun-if-changed={}", CONFIG_FILE);
    let config_string = fs::read_to_string(CONFIG_FILE)
        .unwrap_or_else(|e| panic!("Unable to read {} file: {}", CONFIG_FILE, e));
    let config: Config = serde_yaml::from_str(&config_string)
        .unwrap_or_else(|e| panic!("Unable to parse {} file: {}", CONFIG_FILE, e));

    compile_arduino(&config);
    generate_bindings(&config);
}
