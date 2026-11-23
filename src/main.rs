// B++ 1.4 Windows - PattitexTEC 2026
use eframe::egui;
use std::{
    collections::HashMap,
    fs,
    io::{self, Write},
    process::Command,
    thread,
    time::Duration,
};

const PROYECTOS: &str = "BppProjects";
const LIBS: &str = "BppProjects/LIBS";
const BIN: &str = "BppProjects/BIN";
const MAGIC: &[u8; 4] = b"BPPB";
const VERSION_BIN: u8 = 1;

fn error_msg(lin: usize, detalle: &str) -> String {
    format!("[!] Error en linea {}: {}", lin, detalle)
}

fn ejecutar_comando_gui(
    linea: &str,
    mem: &mut HashMap<String, String>,
    lin: usize,
    libs: &[String],
    salida: &mut String,
) -> Result<(), String> {
    let l = linea.trim();
    if l.is_empty()
        || l.starts_with("//")
        || l == "{"
        || l == "}"
        || l == "["
        || l == "]"
        || l.starts_with("init main")
        || l.starts_with("si_no")
    {
        return Ok(());
    }

    if let Some(cmd_raw) = l.strip_prefix("sistema(\"").and_then(|s| s.strip_suffix("\")")) {
        let cmd = mem
            .iter()
            .fold(cmd_raw.to_string(), |acc, (k, v)| acc.replace(&format!("${}$", k), v));
        match Command::new("cmd").args(["/C", &cmd]).output() {
            Ok(o) => {
                salida.push_str(&String::from_utf8_lossy(&o.stdout));
                salida.push_str(&String::from_utf8_lossy(&o.stderr));
                return Ok(());
            }
            Err(e) => return Err(error_msg(lin, &e.to_string())),
        }
    } else if l.starts_with("sistema(") {
        return Err(error_msg(lin, "Sintaxis invalida en sistema(...)"));
    }

    if l == "limpiar" {
        salida.clear();
        return Ok(());
    }

    if let Some(num) = l.strip_prefix("esperar(").and_then(|s| s.strip_suffix(")")) {
        if let Ok(segs) = num.trim().parse::<f32>() {
            thread::sleep(Duration::from_secs_f32(segs));
            return Ok(());
        }
        return Err(error_msg(lin, "Tiempo invalido en esperar(...)"));
    }

    if let Some(rest) = l.strip_prefix("val ") {
        if let Some((i, d)) = rest.split_once('=') {
            mem.insert(i.trim().to_string(), d.trim().replace('"', ""));
            return Ok(());
        }
        return Err(error_msg(lin, "Sintaxis invalida en val"));
    }

    if let Some(rest) = l.strip_prefix("preguntar(\"") {
        if let Some((preg, dest)) = rest.split_once("\")->") {
            // En GUI tomamos el valor por defecto o vacio si no hay stdin interactivo
            let val = mem.get(dest.trim()).cloned().unwrap_or_default();
            salida.push_str(&format!("{} [auto/prev: {}]\n", preg, val));
            mem.insert(dest.trim().to_string(), val);
            return Ok(());
        }
        return Err(error_msg(lin, "Sintaxis invalida en preguntar"));
    }

    if l.contains("->") && !l.starts_with("preguntar") && !l.starts_with("si ") {
        if let Some((izq, dest)) = l.split_once("->") {
            if let Some((lib_name, exp_raw)) = izq.trim().split_once('(') {
                let lib_name = lib_name.trim();
                if libs.contains(&lib_name.to_string()) {
                    let exp_res = mem
                        .iter()
                        .fold(exp_raw.trim_end_matches(')').to_string(), |acc, (k, v)| {
                            acc.replace(k, v)
                        });
                    let py_cmd = if Command::new("python").arg("--version").output().is_ok() {
                        "python"
                    } else {
                        "py"
                    };
                    match Command::new(py_cmd)
                        .args([&format!("{LIBS}/{lib_name}.py"), &exp_res])
                        .output()
                    {
                        Ok(res) => {
                            mem.insert(
                                dest.trim().to_string(),
                                String::from_utf8_lossy(&res.stdout).trim().to_string(),
                            );
                            return Ok(());
                        }
                        Err(e) => return Err(error_msg(lin, &e.to_string())),
                    }
                }
            }
        }
    }

    if let Some(msg_raw) = l.strip_prefix("text(").and_then(|s| s.strip_suffix(")")) {
        let mt = msg_raw.trim();
        let mut msg = if mt.starts_with('"') && mt.ends_with('"') {
            mt[1..mt.len() - 1].to_string()
        } else {
            mem.get(mt).cloned().unwrap_or_else(|| mt.to_string())
        };
        msg = mem
            .iter()
            .fold(msg, |acc, (k, v)| acc.replace(&format!("${}$", k), v));
        salida.push_str(&format!("{msg}\n"));
        return Ok(());
    } else if l.starts_with("text(") {
        return Err(error_msg(lin, "Sintaxis invalida en text(...)"));
    }

    if let Some(rest) = l.strip_prefix("si ") {
        if let Some((cond, acc)) = rest.split_once("->") {
            if let Some(op) = ["==", "!=", ">=", "<=", ">", "<"]
                .iter()
                .find(|&&o| cond.contains(o))
            {
                if let Some((i, d)) = cond.split_once(op) {
                    let (vr, dc) = (
                        mem.get(i.trim()).cloned().unwrap_or_default(),
                        d.trim().replace('"', ""),
                    );
                    let vf = mem.get(&dc).cloned().unwrap_or(dc);
                    let ok = match *op {
                        "==" => vr == vf,
                        "!=" => vr != vf,
                        _ => false,
                    };
                    if ok {
                        return ejecutar_comando_gui(acc, mem, lin, libs, salida);
                    }
                    return Ok(());
                }
            }
        }
        return Err(error_msg(lin, "Condicion invalida en si"));
    }

    Err(error_msg(lin, &format!("Comando desconocido: {l}")))
}

fn ejecutar_programa_gui(lineas: &[String], salida: &mut String) {
    let (mut mem, mut libs, mut i, mut pila) = (
        HashMap::<String, String>::new(),
        Vec::<String>::new(),
        0usize,
        Vec::<(usize, usize, i32)>::new(),
    );

    while i < lineas.len() {
        let l = lineas[i].trim();
        if let Some(lib_raw) = l.strip_prefix("#import <").and_then(|s| s.strip_suffix(">")) {
            let lib = lib_raw.trim().to_string();
            if !libs.contains(&lib) {
                libs.push(lib);
            }
        } else if let Some(cond_raw) = l.strip_prefix("si ").and_then(|s| s.strip_suffix("[")) {
            let cond = cond_raw.trim();
            let mut cumple = false;
            if let Some(op) = ["==", "!=", ">=", "<=", ">", "<"]
                .iter()
                .find(|&&o| cond.contains(o))
            {
                if let Some((i_str, d)) = cond.split_once(op) {
                    let (vr, dc) = (
                        mem.get(i_str.trim()).cloned().unwrap_or_default(),
                        d.trim().replace('"', ""),
                    );
                    let vf = mem.get(&dc).cloned().unwrap_or(dc);
                    cumple = match *op {
                        "==" => vr == vf,
                        "!=" => vr != vf,
                        _ => false,
                    };
                }
            }
            if !cumple {
                let mut nivel = 1;
                while i + 1 < lineas.len() && nivel > 0 {
                    i += 1;
                    let sig = lineas[i].trim();
                    if sig.ends_with('[') {
                        nivel += 1;
                    } else if sig == "]" {
                        nivel -= 1;
                        if nivel == 0 {
                            if i + 1 < lineas.len() && lineas[i + 1].trim().starts_with("si_no") {
                                i += 1;
                            }
                            break;
                        }
                    }
                }
            }
        } else if l.starts_with("si_no") {
            let mut nivel = 1;
            while i + 1 < lineas.len() && nivel > 0 {
                i += 1;
                let sig = lineas[i].trim();
                if sig.ends_with('[') {
                    nivel += 1;
                } else if sig == "]" {
                    nivel -= 1;
                }
            }
        } else if l.starts_with("Repetir") {
            let mut nivel = 0;
            if let Some(f) = lineas[i + 1..]
                .iter()
                .position(|x| {
                    let xt = x.trim();
                    if xt.starts_with("Repetir") {
                        nivel += 1;
                        false
                    } else if xt == "FB" {
                        if nivel == 0 {
                            true
                        } else {
                            nivel -= 1;
                            false
                        }
                    } else {
                        false
                    }
                })
                .map(|p| p + i + 1)
            {
                let iters = l
                    .split_whitespace()
                    .nth(1)
                    .and_then(|n| n.parse::<i32>().ok())
                    .unwrap_or(0);
                if iters > 0 {
                    pila.push((i + 1, f, iters));
                } else {
                    i = f;
                }
            } else {
                salida.push_str(&error_msg(i + 1, "Falta FB para Repetir\n"));
                break;
            }
        } else if l == "FB" {
            if let Some(mut b) = pila.pop() {
                if b.2 > 1 {
                    b.2 -= 1;
                    pila.push(b);
                    i = b.0;
                    continue;
                }
            }
        } else if let Err(e) = ejecutar_comando_gui(&lineas[i], &mut mem, i + 1, &libs, salida) {
            salida.push_str(&format!("{e}\n"));
            break;
        }
        i += 1;
    }
}

fn compilar_proyecto_win(nombre: &str, lineas: &[String]) -> io::Result<(String, String)> {
    fs::create_dir_all(BIN)?;
    let base = nombre.trim_end_matches(".bpp");
    let ruta_datos = format!("{BIN}/{base}.bin.dat");
    let ruta_bat = format!("{BIN}/{base}.bat");

    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(MAGIC);
    buf.push(VERSION_BIN);
    buf.extend_from_slice(&(lineas.len() as u32).to_le_bytes());
    for l in lineas {
        let bytes = l.as_bytes();
        buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(bytes);
    }
    fs::write(&ruta_datos, buf)?;

    let exe = std::env::current_exe()?;
    let exe_str = exe.display().to_string();
    let datos_abs = fs::canonicalize(&ruta_datos)
        .unwrap_or_else(|_| std::path::PathBuf::from(&ruta_datos))
        .display()
        .to_string();

    // Script .bat para Windows
    let bat = format!(
        "@echo off\r\ncd /d \"%~dp0..\\..\"\r\n\"{}\" --run \"{}\"\r\npause\r\n",
        exe_str, datos_abs
    );
    fs::write(&ruta_bat, bat)?;
    Ok((ruta_bat, ruta_datos))
}

fn leer_bin(ruta: &str) -> io::Result<Vec<String>> {
    let data = fs::read(ruta)?;
    if data.len() < 9 || &data[0..4] != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Archivo .bin.dat invalido o corrupto",
        ));
    }
    if data[4] != VERSION_BIN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Version de binario incompatible",
        ));
    }
    let mut pos = 5;
    let n_lineas = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
    pos += 4;
    let mut lineas = Vec::with_capacity(n_lineas);
    for _ in 0..n_lineas {
        if pos + 4 > data.len() {
            break;
        }
        let len = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        if pos + len > data.len() {
            break;
        }
        lineas.push(String::from_utf8_lossy(&data[pos..pos + len]).into_owned());
        pos += len;
    }
    Ok(lineas)
}

fn listar_proyectos() -> Vec<String> {
    fs::read_dir(PROYECTOS)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".bpp"))
        .collect()
}

struct BppApp {
    proyectos: Vec<String>,
    actual: Option<String>,
    codigo: String,
    nuevo_nombre: String,
    coffi_pkg: String,
    consola: String,
}

impl Default for BppApp {
    fn default() -> Self {
        let _ = fs::create_dir_all(LIBS);
        let _ = fs::create_dir_all(BIN);
        let proyectos = listar_proyectos();
        let actual = proyectos.first().cloned();
        let codigo = actual
            .as_ref()
            .and_then(|n| fs::read_to_string(format!("{PROYECTOS}/{n}")).ok())
            .unwrap_or_else(|| "init main {\n  text(\"Hola desde B++ Windows!\")\n}".into());
        Self {
            proyectos,
            actual,
            codigo,
            nuevo_nombre: String::new(),
            coffi_pkg: String::new(),
            consola: String::from("B++ v1.4 Windows IDE - PattitexTEC\n"),
        }
    }
}

impl eframe::App for BppApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("B++ v1.4 Windows");
                ui.separator();
                if ui.button("Guardar").clicked() {
                    if let Some(ref nom) = self.actual {
                        let _ = fs::write(format!("{PROYECTOS}/{nom}"), &self.codigo);
                        self.consola.push_str(&format!("[OK] Guardado {nom}\n"));
                    }
                }
                if ui.button("Compilar + Ejecutar").clicked() {
                    if let Some(ref nom) = self.actual {
                        let _ = fs::write(format!("{PROYECTOS}/{nom}"), &self.codigo);
                        let lineas: Vec<String> = self.codigo.lines().map(String::from).collect();
                        match compilar_proyecto_win(nom, &lineas) {
                            Ok((bat, dat)) => {
                                self.consola
                                    .push_str(&format!("[Compilador] Generado: {bat}\n"));
                                if let Ok(leidas) = leer_bin(&dat) {
                                    ejecutar_programa_gui(&leidas, &mut self.consola);
                                }
                            }
                            Err(e) => {
                                self.consola.push_str(&format!("[!] Error compilar: {e}\n"));
                            }
                        }
                    }
                }
                if ui.button("Limpiar consola").clicked() {
                    self.consola.clear();
                }
            });
        });

        egui::SidePanel::left("proyectos_panel")
            .resizable(true)
            .default_width(210.0)
            .show(ctx, |ui| {
                ui.label("Proyectos (.bpp)");
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.nuevo_nombre);
                    if ui.button("Nuevo").clicked() && !self.nuevo_nombre.trim().is_empty() {
                        let raw = self.nuevo_nombre.trim();
                        let nom = if raw.ends_with(".bpp") {
                            raw.to_string()
                        } else {
                            format!("{raw}.bpp")
                        };
                        let base_code = "init main {\n  \n}\n";
                        let _ = fs::write(format!("{PROYECTOS}/{nom}"), base_code);
                        self.proyectos = listar_proyectos();
                        self.actual = Some(nom);
                        self.codigo = base_code.into();
                        self.nuevo_nombre.clear();
                    }
                });
                ui.separator();
                for p in self.proyectos.clone() {
                    let selected = self.actual.as_deref() == Some(p.as_str());
                    if ui.selectable_label(selected, &p).clicked() {
                        self.actual = Some(p.clone());
                        self.codigo =
                            fs::read_to_string(format!("{PROYECTOS}/{p}")).unwrap_or_default();
                    }
                }
                ui.separator();
                ui.label("Coffi Package Manager");
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.coffi_pkg);
                    if ui.button("Instalar").clicked() && !self.coffi_pkg.trim().is_empty() {
                        let pkg = self.coffi_pkg.trim().to_string();
                        self.consola.push_str(&format!("[Coffi] Buscando '{pkg}'...\n"));
                        let url = "https://raw.githubusercontent.com/loslocos817yt-star/Coffi-Packer-Manager/refs/heads/main/Librerias/Oficiales.json";
                        if let Ok(out) = Command::new("curl").args(["-s", url]).output() {
                            let body = String::from_utf8_lossy(&out.stdout);
                            let mut iter = body.lines();
                            let mut found_link = None;
                            while let Some(l) = iter.next() {
                                if l.contains("\"paquete\"") {
                                    if let Some(n) = l.split('"').nth(3) {
                                        if n.eq_ignore_ascii_case(&pkg) {
                                            if let Some(u) = iter
                                                .find(|x| x.contains("\"link\""))
                                                .and_then(|x| x.split('"').nth(3))
                                            {
                                                found_link = Some((n.to_string(), u.to_string()));
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                            if let Some((real, link)) = found_link {
                                let dest = format!("{LIBS}/{real}.py");
                                let ok = Command::new("curl")
                                    .args(["-s", "-o", &dest, &link])
                                    .status()
                                    .map(|s| s.success())
                                    .unwrap_or(false);
                                if ok {
                                    self.consola.push_str(&format!("[Coffi] Instalado {dest}\n"));
                                } else {
                                    self.consola.push_str("[Coffi] Error al descargar.\n");
                                }
                            } else {
                                self.consola.push_str(&format!("[Coffi] No encontrado '{pkg}'.\n"));
                            }
                        } else {
                            self.consola.push_str("[Coffi] Error ejecutando curl.\n");
                        }
                    }
                });
            });

        egui::TopBottomPanel::bottom("console_panel")
            .resizable(true)
            .default_height(180.0)
            .show(ctx, |ui| {
                ui.label("Salida / Consola");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.consola)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY),
                    );
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            let titulo = self.actual.as_deref().unwrap_or("Sin archivo");
            ui.label(format!("Editor: {titulo}"));
            egui::ScrollArea::both().show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.codigo)
                        .font(egui::TextStyle::Monospace)
                        .code_editor()
                        .desired_rows(22)
                        .desired_width(f32::INFINITY),
                );
            });
        });
    }
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 && args[1] == "--run" {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let _ = std::env::set_current_dir(dir);
            }
        }
        match leer_bin(&args[2]) {
            Ok(lineas) => {
                let mut out = String::new();
                ejecutar_programa_gui(&lineas, &mut out);
                print!("{out}");
                let _ = io::stdout().flush();
            }
            Err(e) => eprintln!("[!] Error al ejecutar binario: {e}"),
        }
        return Ok(());
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([920.0, 620.0]),
        ..Default::default()
    };
    eframe::run_native(
        "B++ v1.4 Windows",
        options,
        Box::new(|_cc| Ok(Box::new(BppApp::default()))),
    )
}
