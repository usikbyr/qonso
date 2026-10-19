use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tower_http::cors::{Any, CorsLayer};

pub type ElementId = usize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ElementType {
    Base,
    Consolidated { children: Vec<(ElementId, f64)> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dimension {
    pub name: String,
    pub elements: Vec<ElementType>,
    pub name_to_id: HashMap<String, ElementId>,
    pub id_to_name: Vec<String>,
}

impl Dimension {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            elements: Vec::new(),
            name_to_id: HashMap::new(),
            id_to_name: Vec::new(),
        }
    }

    pub fn add_base_element(&mut self, name: &str) -> ElementId {
        if let Some(&id) = self.name_to_id.get(name) {
            return id;
        }
        let id = self.elements.len();
        self.elements.push(ElementType::Base);
        self.name_to_id.insert(name.to_string(), id);
        self.id_to_name.push(name.to_string());
        id
    }

    pub fn add_consolidated_element(&mut self, name: &str, children: Vec<(&str, f64)>) -> ElementId {
        let mut resolved = Vec::new();
        for (c_name, weight) in children {
            let c_id = self.name_to_id.get(c_name).cloned().unwrap_or_else(|| {
                panic!("Enfant inconnu '{}' pour '{}'", c_name, name)
            });
            resolved.push((c_id, weight));
        }

        let id = self.elements.len();
        self.elements.push(ElementType::Consolidated { children: resolved });
        self.name_to_id.insert(name.to_string(), id);
        self.id_to_name.push(name.to_string());
        id
    }

    pub fn get_id(&self, name: &str) -> Option<ElementId> {
        self.name_to_id.get(name).copied()
    }

    pub fn get_name(&self, id: ElementId) -> &str {
        &self.id_to_name[id]
    }
}

pub type Coordinate = Vec<ElementId>;

/// Types d'opérations pour les règles de calcul non additives (Chantier B)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleOp {
    Divide,
    Multiply,
    Percentage, // (A / B) * 100
}

/// Règle de calcul sur une dimension cible (ex: Measures: AvgPrice = Revenue / Qty)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub dim_index: usize,
    pub target_elem: ElementId,
    pub num_elem: ElementId,
    pub den_elem: ElementId,
    pub op: RuleOp,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Cube {
    pub name: String,
    pub dimensions: Vec<Dimension>,
    pub base_cells: HashMap<Coordinate, f64>,
    pub rules: Vec<Rule>,
}

impl Cube {
    pub fn new(name: &str, dimensions: Vec<Dimension>) -> Self {
        Self {
            name: name.to_string(),
            dimensions,
            base_cells: HashMap::new(),
            rules: Vec::new(),
        }
    }

    pub fn add_ratio_rule(
        &mut self,
        dim_name: &str,
        target_name: &str,
        num_name: &str,
        den_name: &str,
        op: RuleOp,
    ) -> Result<(), String> {
        let dim_idx = self
            .dimensions
            .iter()
            .position(|d| d.name == dim_name)
            .ok_or_else(|| format!("Dimension '{}' introuvable", dim_name))?;

        let target_id = self.dimensions[dim_idx]
            .get_id(target_name)
            .ok_or_else(|| format!("Élément '{}' introuvable", target_name))?;
        let num_id = self.dimensions[dim_idx]
            .get_id(num_name)
            .ok_or_else(|| format!("Élément '{}' introuvable", num_name))?;
        let den_id = self.dimensions[dim_idx]
            .get_id(den_name)
            .ok_or_else(|| format!("Élément '{}' introuvable", den_name))?;

        self.rules.push(Rule {
            dim_index: dim_idx,
            target_elem: target_id,
            num_elem: num_id,
            den_elem: den_id,
            op,
        });

        Ok(())
    }

    pub fn resolve_coord(&self, names: &[&str]) -> Result<Coordinate, String> {
        if names.len() != self.dimensions.len() {
            return Err(format!(
                "Nombre d'éléments mismatch : attendu {}, reçu {}",
                self.dimensions.len(),
                names.len()
            ));
        }
        let mut coord = Vec::new();
        for (idx, name) in names.iter().enumerate() {
            let id = self.dimensions[idx].get_id(name).ok_or_else(|| {
                format!("Élément '{}' inconnu dans dimension '{}'", name, self.dimensions[idx].name)
            })?;
            coord.push(id);
        }
        Ok(coord)
    }

    pub fn set_base_value(&mut self, coord: &Coordinate, value: f64) -> Result<(), String> {
        for (dim_idx, &elem_id) in coord.iter().enumerate() {
            // Vérifier si une règle s'applique à cet élément
            for r in &self.rules {
                if r.dim_index == dim_idx && r.target_elem == elem_id {
                    return Err(format!(
                        "L'élément '{}' est calculé par une règle métier. Saisie interdite.",
                        self.dimensions[dim_idx].get_name(elem_id)
                    ));
                }
            }

            if let ElementType::Consolidated { .. } = &self.dimensions[dim_idx].elements[elem_id] {
                return Err(format!(
                    "L'élément '{}' est consolidé. Utilisez le splashing.",
                    self.dimensions[dim_idx].get_name(elem_id)
                ));
            }
        }

        if value == 0.0 {
            self.base_cells.remove(coord);
        } else {
            self.base_cells.insert(coord.clone(), value);
        }
        Ok(())
    }

    pub fn get_value(&self, coord: &Coordinate) -> f64 {
        // 1. Évaluation prioritaire des règles métiers (Rules Engine)
        for rule in &self.rules {
            if coord[rule.dim_index] == rule.target_elem {
                let mut num_coord = coord.clone();
                num_coord[rule.dim_index] = rule.num_elem;
                let num_val = self.get_value(&num_coord);

                let mut den_coord = coord.clone();
                den_coord[rule.dim_index] = rule.den_elem;
                let den_val = self.get_value(&den_coord);

                if den_val == 0.0 {
                    return 0.0;
                }

                return match rule.op {
                    RuleOp::Divide => num_val / den_val,
                    RuleOp::Multiply => num_val * den_val,
                    RuleOp::Percentage => (num_val / den_val) * 100.0,
                };
            }
        }

        // 2. Évaluation des consolidations hiérarchiques (Roll-up)
        for (dim_idx, &elem_id) in coord.iter().enumerate() {
            if let ElementType::Consolidated { children } = &self.dimensions[dim_idx].elements[elem_id] {
                let mut sum = 0.0;
                let mut sub_coord = coord.clone();
                for &(child_id, weight) in children {
                    sub_coord[dim_idx] = child_id;
                    sum += weight * self.get_value(&sub_coord);
                }
                return sum;
            }
        }

        // 3. Lecture cellule de base
        *self.base_cells.get(coord).unwrap_or(&0.0)
    }

    pub fn splash_set(&mut self, coord: &Coordinate, target_value: f64) -> Result<(), String> {
        let current_value = self.get_value(coord);
        let leaves = self.collect_base_cells(coord);

        if leaves.is_empty() {
            return Err("Aucune feuille trouvée".to_string());
        }

        if current_value == 0.0 {
            let share = target_value / leaves.len() as f64;
            for leaf in leaves {
                self.set_base_value(&leaf, share)?;
            }
        } else {
            let ratio = target_value / current_value;
            for leaf in leaves {
                let prev = *self.base_cells.get(&leaf).unwrap_or(&0.0);
                self.set_base_value(&leaf, prev * ratio)?;
            }
        }
        Ok(())
    }

    fn collect_base_cells(&self, coord: &Coordinate) -> Vec<Coordinate> {
        for (dim_idx, &elem_id) in coord.iter().enumerate() {
            if let ElementType::Consolidated { children } = &self.dimensions[dim_idx].elements[elem_id] {
                let mut result = Vec::new();
                let mut sub_coord = coord.clone();
                for &(child_id, _) in children {
                    sub_coord[dim_idx] = child_id;
                    result.extend(self.collect_base_cells(&sub_coord));
                }
                return result;
            }
        }
        vec![coord.clone()]
    }
}

// ----------------------------------------------------
// Chantier C : Moteur Multi-Cubes & Persistance (WAL / Snapshot)
// ----------------------------------------------------
#[derive(Serialize, Deserialize)]
pub struct WalEntry {
    pub cube_name: String,
    pub coordinates: Vec<String>,
    pub value: f64,
}

pub struct CubeServer {
    pub cubes: HashMap<String, Cube>,
    pub data_dir: PathBuf,
}

impl CubeServer {
    pub fn new(data_dir: &str) -> Self {
        let dir = PathBuf::from(data_dir);
        let _ = fs::create_dir_all(&dir);
        Self {
            cubes: HashMap::new(),
            data_dir: dir,
        }
    }

    pub fn add_cube(&mut self, cube: Cube) {
        self.cubes.insert(cube.name.clone(), cube);
    }

    pub fn get_cube(&self, name: &str) -> Option<&Cube> {
        let normalized = if name.eq_ignore_ascii_case("CUBE_RETROGAMING_SHOP") {
            "RetroGamingShop"
        } else {
            name
        };
        self.cubes.get(normalized)
    }

    pub fn get_cube_mut(&mut self, name: &str) -> Option<&mut Cube> {
        let normalized = if name.eq_ignore_ascii_case("CUBE_RETROGAMING_SHOP") {
            "RetroGamingShop"
        } else {
            name
        };
        self.cubes.get_mut(normalized)
    }

    /// Écrit une transaction dans le journal WAL (disque)
    pub fn append_wal(&self, entry: &WalEntry) -> Result<(), String> {
        let wal_path = self.data_dir.join("qonso_wal.log");
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&wal_path)
            .map_err(|e| e.to_string())?;

        let json = serde_json::to_string(entry).map_err(|e| e.to_string())?;
        writeln!(file, "{}", json).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Rejoue les transactions du WAL lors du démarrage
    pub fn replay_wal(&mut self) -> Result<usize, String> {
        let wal_path = self.data_dir.join("qonso_wal.log");
        if !wal_path.exists() {
            return Ok(0);
        }

        let file = File::open(&wal_path).map_err(|e| e.to_string())?;
        let reader = BufReader::new(file);
        let mut count = 0;

        for line in reader.lines() {
            if let Ok(l) = line {
                if l.trim().is_empty() {
                    continue;
                }
                if let Ok(entry) = serde_json::from_str::<WalEntry>(&l) {
                    if let Some(cube) = self.cubes.get_mut(&entry.cube_name) {
                        let str_coords: Vec<&str> =
                            entry.coordinates.iter().map(|s| s.as_str()).collect();
                        if let Ok(coord) = cube.resolve_coord(&str_coords) {
                            let _ = cube.set_base_value(&coord, entry.value);
                            count += 1;
                        }
                    }
                }
            }
        }
        Ok(count)
    }

    /// Sauvegarde snapshot complet de tous les cubes en JSON
    pub fn save_snapshot(&self) -> Result<(), String> {
        let snap_path = self.data_dir.join("snapshot_cubes.json");
        let json = serde_json::to_string_pretty(&self.cubes).map_err(|e| e.to_string())?;
        fs::write(snap_path, json).map_err(|e| e.to_string())?;
        Ok(())
    }
}

// ----------------------------------------------------
// Base de données "Retro Gaming Shop" & Cube Secondaire "HR"
// ----------------------------------------------------
pub fn build_retro_shop_cube() -> Cube {
    let mut d_time = Dimension::new("Period");
    d_time.add_base_element("2024-01");
    d_time.add_base_element("2024-02");
    d_time.add_base_element("2024-03");
    d_time.add_consolidated_element("2024-Q1", vec![("2024-01", 1.0), ("2024-02", 1.0), ("2024-03", 1.0)]);
    d_time.add_base_element("2024-04");
    d_time.add_base_element("2024-05");
    d_time.add_base_element("2024-06");
    d_time.add_consolidated_element("2024-Q2", vec![("2024-04", 1.0), ("2024-05", 1.0), ("2024-06", 1.0)]);
    d_time.add_consolidated_element("2024-Total", vec![("2024-Q1", 1.0), ("2024-Q2", 1.0)]);

    let mut d_prod = Dimension::new("Products");
    d_prod.add_base_element("NES");
    d_prod.add_base_element("SNES");
    d_prod.add_base_element("Megadrive");
    d_prod.add_consolidated_element("Consoles-Salon", vec![("NES", 1.0), ("SNES", 1.0), ("Megadrive", 1.0)]);
    d_prod.add_base_element("GameBoy");
    d_prod.add_base_element("GameGear");
    d_prod.add_consolidated_element("Consoles-Portables", vec![("GameBoy", 1.0), ("GameGear", 1.0)]);
    d_prod.add_base_element("Amiga 500");
    d_prod.add_base_element("Atari ST");
    d_prod.add_consolidated_element("Micro-Retro", vec![("Amiga 500", 1.0), ("Atari ST", 1.0)]);
    d_prod.add_consolidated_element(
        "Total-Catalogue",
        vec![("Consoles-Salon", 1.0), ("Consoles-Portables", 1.0), ("Micro-Retro", 1.0)],
    );

    let mut d_channel = Dimension::new("Channels");
    d_channel.add_base_element("Boutique-Paris");
    d_channel.add_base_element("Boutique-Lyon");
    d_channel.add_consolidated_element("Boutiques-Physiques", vec![("Boutique-Paris", 1.0), ("Boutique-Lyon", 1.0)]);
    d_channel.add_base_element("Site-Web");
    d_channel.add_consolidated_element("Tous-Canaux", vec![("Boutiques-Physiques", 1.0), ("Site-Web", 1.0)]);

    // Dimension Indicateurs Financiers avec Règles Métiers (Chantier B)
    let mut d_measures = Dimension::new("Measures");
    d_measures.add_base_element("Qty");         // Unités
    d_measures.add_base_element("Revenue");     // CA
    d_measures.add_base_element("COGS");        // Coût
    d_measures.add_consolidated_element("Margin", vec![("Revenue", 1.0), ("COGS", -1.0)]); // Marge brute
    d_measures.add_base_element("AvgPrice");    // Règle: Revenue / Qty
    d_measures.add_base_element("MarginPct");   // Règle: (Margin / Revenue) * 100

    let mut cube = Cube::new("RetroGamingShop", vec![d_time, d_prod, d_channel, d_measures]);

    // Enregistrement des règles métiers dans le Cube (Chantier B)
    let _ = cube.add_ratio_rule("Measures", "AvgPrice", "Revenue", "Qty", RuleOp::Divide);
    let _ = cube.add_ratio_rule("Measures", "MarginPct", "Margin", "Revenue", RuleOp::Percentage);

    // Chargement de ventes réelles (Base Data)
    let sales = vec![
        ("2024-01", "NES", "Boutique-Paris", 12.0, 1440.0, 720.0),
        ("2024-01", "SNES", "Boutique-Paris", 15.0, 2250.0, 1200.0),
        ("2024-01", "Megadrive", "Boutique-Lyon", 8.0, 960.0, 480.0),
        ("2024-01", "GameBoy", "Site-Web", 35.0, 2800.0, 1400.0),
        ("2024-01", "Amiga 500", "Site-Web", 4.0, 1800.0, 1000.0),
        ("2024-02", "NES", "Boutique-Lyon", 10.0, 1200.0, 600.0),
        ("2024-02", "SNES", "Site-Web", 22.0, 3300.0, 1760.0),
        ("2024-02", "GameGear", "Boutique-Paris", 6.0, 540.0, 300.0),
        ("2024-02", "Atari ST", "Site-Web", 5.0, 1750.0, 950.0),
        ("2024-03", "SNES", "Boutique-Paris", 18.0, 2700.0, 1440.0),
        ("2024-03", "GameBoy", "Boutique-Lyon", 25.0, 2000.0, 1000.0),
        ("2024-03", "Megadrive", "Site-Web", 14.0, 1680.0, 840.0),
        ("2024-03", "Amiga 500", "Boutique-Paris", 3.0, 1350.0, 750.0),
        ("2024-04", "NES", "Site-Web", 15.0, 1800.0, 900.0),
        ("2024-04", "GameBoy", "Site-Web", 40.0, 3200.0, 1600.0),
        ("2024-05", "SNES", "Boutique-Lyon", 12.0, 1800.0, 960.0),
        ("2024-05", "Atari ST", "Site-Web", 6.0, 2100.0, 1140.0),
        ("2024-06", "GameBoy", "Boutique-Paris", 30.0, 2400.0, 1200.0),
        ("2024-06", "Megadrive", "Boutique-Paris", 9.0, 1080.0, 540.0),
        ("2024-06", "Amiga 500", "Site-Web", 5.0, 2250.0, 1250.0),
    ];

    for (p, prod, chan, qty, rev, cogs) in sales {
        let coord_qty = cube.resolve_coord(&[p, prod, chan, "Qty"]).unwrap();
        let coord_rev = cube.resolve_coord(&[p, prod, chan, "Revenue"]).unwrap();
        let coord_cogs = cube.resolve_coord(&[p, prod, chan, "COGS"]).unwrap();

        let _ = cube.set_base_value(&coord_qty, qty);
        let _ = cube.set_base_value(&coord_rev, rev);
        let _ = cube.set_base_value(&coord_cogs, cogs);
    }

    cube
}

// ----------------------------------------------------
// DTOs & API Web (Axum)
// ----------------------------------------------------
#[derive(Serialize)]
struct DimensionDto {
    name: String,
    elements: Vec<ElementDto>,
}

#[derive(Serialize)]
struct ElementDto {
    name: String,
    is_consolidated: bool,
    children: Vec<String>,
}

#[derive(Serialize)]
struct MetadataResponse {
    cube_name: String,
    cubes_list: Vec<String>,
    dimensions: Vec<DimensionDto>,
}

#[derive(Deserialize)]
struct QueryCellRequest {
    cube_name: Option<String>,
    coordinates: Vec<String>,
}

#[derive(Serialize)]
struct QueryCellResponse {
    coordinates: Vec<String>,
    value: f64,
}

#[derive(Deserialize)]
struct PivotGridRequest {
    cube_name: Option<String>,
    row_dim: String,
    col_dim: String,
    row_elements: Vec<String>,
    col_elements: Vec<String>,
    filters: HashMap<String, String>,
}

#[derive(Serialize)]
struct PivotGridResponse {
    row_dim: String,
    col_dim: String,
    col_elements: Vec<String>,
    rows: Vec<PivotRow>,
}

#[derive(Serialize)]
struct PivotRow {
    row_element: String,
    values: Vec<f64>,
}

#[derive(Deserialize)]
struct SplashRequest {
    cube_name: Option<String>,
    coordinates: Vec<String>,
    target_value: f64,
}

type AppState = Arc<RwLock<CubeServer>>;

async fn list_cubes(State(state): State<AppState>) -> Json<Vec<String>> {
    let server = state.read().unwrap();
    Json(server.cubes.keys().cloned().collect())
}

async fn get_metadata(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Result<Json<MetadataResponse>, (StatusCode, String)> {
    let server = state.read().unwrap();
    let cube_name = params
        .get("cube")
        .cloned()
        .unwrap_or_else(|| "RetroGamingShop".to_string());

    let cube = server
        .get_cube(&cube_name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Cube introuvable".to_string()))?;

    let dims: Vec<DimensionDto> = cube
        .dimensions
        .iter()
        .map(|d| {
            let elements = d
                .id_to_name
                .iter()
                .enumerate()
                .map(|(idx, name)| {
                    let (is_consolidated, children) = match &d.elements[idx] {
                        ElementType::Base => (false, vec![]),
                        ElementType::Consolidated { children } => (
                            true,
                            children.iter().map(|&(c, _)| d.id_to_name[c].clone()).collect(),
                        ),
                    };
                    ElementDto {
                        name: name.clone(),
                        is_consolidated,
                        children,
                    }
                })
                .collect();
            DimensionDto {
                name: d.name.clone(),
                elements,
            }
        })
        .collect();

    Ok(Json(MetadataResponse {
        cube_name: cube.name.clone(),
        cubes_list: server.cubes.keys().cloned().collect(),
        dimensions: dims,
    }))
}

async fn query_cell(
    State(state): State<AppState>,
    Json(payload): Json<QueryCellRequest>,
) -> Result<Json<QueryCellResponse>, (StatusCode, String)> {
    let server = state.read().unwrap();
    let cube_name = payload
        .cube_name
        .unwrap_or_else(|| "RetroGamingShop".to_string());
    let cube = server
        .get_cube(&cube_name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Cube non trouvé".to_string()))?;

    let str_coords: Vec<&str> = payload.coordinates.iter().map(|s| s.as_str()).collect();
    let c = cube.resolve_coord(&str_coords).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let val = cube.get_value(&c);
    Ok(Json(QueryCellResponse {
        coordinates: payload.coordinates,
        value: val,
    }))
}

async fn pivot_grid(
    State(state): State<AppState>,
    Json(payload): Json<PivotGridRequest>,
) -> Result<Json<PivotGridResponse>, (StatusCode, String)> {
    let server = state.read().unwrap();
    let cube_name = payload
        .cube_name
        .unwrap_or_else(|| "RetroGamingShop".to_string());
    let cube = server
        .get_cube(&cube_name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Cube non trouvé".to_string()))?;

    let mut rows = Vec::new();
    let dim_names: Vec<String> = cube.dimensions.iter().map(|d| d.name.clone()).collect();

    for row_elem in &payload.row_elements {
        let mut row_values = Vec::new();

        for col_elem in &payload.col_elements {
            let mut coord_names = Vec::new();
            for dim_name in &dim_names {
                if dim_name == &payload.row_dim {
                    coord_names.push(row_elem.as_str());
                } else if dim_name == &payload.col_dim {
                    coord_names.push(col_elem.as_str());
                } else if let Some(filtered) = payload.filters.get(dim_name) {
                    coord_names.push(filtered.as_str());
                } else {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        format!("Dimension '{}' non spécifiée dans le filtre", dim_name),
                    ));
                }
            }

            let c = cube
                .resolve_coord(&coord_names)
                .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
            row_values.push(cube.get_value(&c));
        }

        rows.push(PivotRow {
            row_element: row_elem.clone(),
            values: row_values,
        });
    }

    Ok(Json(PivotGridResponse {
        row_dim: payload.row_dim,
        col_dim: payload.col_dim,
        col_elements: payload.col_elements,
        rows,
    }))
}

async fn splash_cell(
    State(state): State<AppState>,
    Json(payload): Json<SplashRequest>,
) -> Result<Json<QueryCellResponse>, (StatusCode, String)> {
    let mut server = state.write().unwrap();
    let cube_name = payload
        .cube_name
        .unwrap_or_else(|| "RetroGamingShop".to_string());

    let wal_entry = WalEntry {
        cube_name: cube_name.clone(),
        coordinates: payload.coordinates.clone(),
        value: payload.target_value,
    };

    let cube = server
        .get_cube_mut(&cube_name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Cube non trouvé".to_string()))?;

    let str_coords: Vec<&str> = payload.coordinates.iter().map(|s| s.as_str()).collect();
    let c = cube.resolve_coord(&str_coords).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    cube.splash_set(&c, payload.target_value).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let new_val = cube.get_value(&c);

    // Persistance dans le WAL sur disque
    let _ = server.append_wal(&wal_entry);

    Ok(Json(QueryCellResponse {
        coordinates: payload.coordinates,
        value: new_val,
    }))
}

#[tokio::main]
async fn main() {
    let mut server = CubeServer::new("data");

    // 1. Initialiser Cube principal Retro Gaming Shop
    let retro_cube = build_retro_shop_cube();
    server.add_cube(retro_cube);

    // 2. Initialiser Cube RH / Salaires (Démonstration Multi-Cubes Chantier C)
    let mut d_period_hr = Dimension::new("Period");
    d_period_hr.add_base_element("2024-01");
    d_period_hr.add_base_element("2024-02");
    d_period_hr.add_base_element("2024-03");

    let mut d_dept = Dimension::new("Department");
    d_dept.add_base_element("Tech");
    d_dept.add_base_element("Ventes");
    d_dept.add_base_element("Support");
    d_dept.add_consolidated_element("Total-Entreprise", vec![("Tech", 1.0), ("Ventes", 1.0), ("Support", 1.0)]);

    let mut d_hr_meas = Dimension::new("HRMeasures");
    d_hr_meas.add_base_element("Headcount");
    d_hr_meas.add_base_element("SalaryCost");

    let mut hr_cube = Cube::new("HR_Salaries", vec![d_period_hr, d_dept, d_hr_meas]);
    if let (Ok(c1), Ok(c2)) = (
        hr_cube.resolve_coord(&["2024-01", "Tech", "SalaryCost"]),
        hr_cube.resolve_coord(&["2024-01", "Tech", "Headcount"]),
    ) {
        let _ = hr_cube.set_base_value(&c1, 15000.0);
        let _ = hr_cube.set_base_value(&c2, 3.0);
    }
    server.add_cube(hr_cube);

    // 3. Rejouer le journal WAL s'il existe
    if let Ok(replayed) = server.replay_wal() {
        if replayed > 0 {
            println!("  [WAL] {} transactions rejouées depuis le disque.", replayed);
        }
    }

    let state = Arc::new(RwLock::new(server));

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let dist_path = std::path::Path::new("frontend/dist");
    let app = if dist_path.exists() {
        Router::new()
            .route("/api/cubes", get(list_cubes))
            .route("/api/metadata", get(get_metadata))
            .route("/api/cell", post(query_cell))
            .route("/api/pivot", post(pivot_grid))
            .route("/api/splash", post(splash_cell))
            .fallback_service(tower_http::services::ServeDir::new(dist_path))
            .layer(cors)
            .with_state(state)
    } else {
        Router::new()
            .route("/api/cubes", get(list_cubes))
            .route("/api/metadata", get(get_metadata))
            .route("/api/cell", post(query_cell))
            .route("/api/pivot", post(pivot_grid))
            .route("/api/splash", post(splash_cell))
            .layer(cors)
            .with_state(state)
    };

    let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:3001".to_string());
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .unwrap();

    println!("====================================================");
    println!("  QONSO MOLAP Server démarré sur http://127.0.0.1:3001");
    println!("  Cubes disponibles: ['RetroGamingShop', 'HR_Salaries']");
    println!("  Moteur de Règles (Rules Engine) activé (AvgPrice, MarginPct)");
    println!("  Persistance WAL & Snapshots activée dans ./data/");
    println!("====================================================");

    axum::serve(listener, app).await.unwrap();
}
