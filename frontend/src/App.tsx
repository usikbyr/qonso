import { useEffect, useState } from "react";
import {
  Layers,
  Database,
  RefreshCw,
  Sliders,
  DollarSign,
  TrendingUp,
  Package,
  Store,
  Calendar,
  Sparkles,
  Table,
} from "lucide-react";
import "./App.css";
import { IronCalcSheet } from "./IronCalcSheet";

interface ElementDto {
  name: string;
  is_consolidated: boolean;
  children: string[];
}

interface DimensionDto {
  name: string;
  elements: ElementDto[];
}

interface MetadataResponse {
  cube_name: string;
  cubes_list: string[];
  dimensions: DimensionDto[];
}

interface PivotRow {
  row_element: string;
  values: number[];
}

interface PivotGridResponse {
  row_dim: string;
  col_dim: string;
  col_elements: string[];
  rows: PivotRow[];
}

const API_BASE = import.meta.env.VITE_API_URL || "/api";

export default function App() {
  const [metadata, setMetadata] = useState<MetadataResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [currentCube, setCurrentCube] = useState<string>("RetroGamingShop");
  const [rowDim, setRowDim] = useState<string>("Products");
  const [colDim, setColDim] = useState<string>("Period");
  const [filters, setFilters] = useState<Record<string, string>>({
    Channels: "Tous-Canaux",
    Measures: "Revenue",
  });

  const [pivotData, setPivotData] = useState<PivotGridResponse | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [activeTab, setActiveTab] = useState<"pivot" | "sheet">("pivot");

  // Modal / Prompt Splashing
  const [splashCoord, setSplashCoord] = useState<{
    row: string;
    col: string;
    currentVal: number;
  } | null>(null);
  const [splashInput, setSplashInput] = useState<string>("");

  // 1. Fetch Metadata
  const fetchMetadata = async (targetCube = currentCube) => {
    try {
      setLoading(true);
      const res = await fetch(`${API_BASE}/metadata?cube=${targetCube}`);
      if (!res.ok) throw new Error("Impossible de joindre le serveur MOLAP Qonso");
      const data: MetadataResponse = await res.json();
      setMetadata(data);
      if (data.dimensions.length >= 2) {
        setRowDim(data.dimensions[0].name);
        setColDim(data.dimensions[1].name);
        const newFilters: Record<string, string> = {};
        for (let i = 2; i < data.dimensions.length; i++) {
          newFilters[data.dimensions[i].name] = data.dimensions[i].elements[0]?.name || "";
        }
        setFilters(newFilters);
      }
      setError(null);
    } catch (err: any) {
      setError(err.message || "Erreur de connexion");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchMetadata(currentCube);
  }, [currentCube]);

  // 2. Fetch Pivot Grid Data
  const fetchPivot = async () => {
    if (!metadata) return;
    setRefreshing(true);

    const rowDimObj = metadata.dimensions.find((d) => d.name === rowDim);
    const colDimObj = metadata.dimensions.find((d) => d.name === colDim);

    if (!rowDimObj || !colDimObj) return;

    try {
      const res = await fetch(`${API_BASE}/pivot`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          cube_name: currentCube,
          row_dim: rowDim,
          col_dim: colDim,
          row_elements: rowDimObj.elements.map((e) => e.name),
          col_elements: colDimObj.elements.map((e) => e.name),
          filters,
        }),
      });

      if (!res.ok) {
        const text = await res.text();
        throw new Error(text || "Erreur lors du calcul multidimensionnel");
      }

      const grid: PivotGridResponse = await res.json();
      setPivotData(grid);
    } catch (err: any) {
      setError(err.message);
    } finally {
      setRefreshing(false);
    }
  };

  useEffect(() => {
    if (metadata) {
      fetchPivot();
    }
  }, [metadata, rowDim, colDim, filters]);

  // Exécution d'un Splash (Write-back)
  const handleApplySplash = async () => {
    if (!splashCoord || !metadata) return;
    const targetVal = parseFloat(splashInput);
    if (isNaN(targetVal)) return;

    // Reconstruire les coordonnées dans l'ordre exact des dimensions du Cube
    const coords: string[] = metadata.dimensions.map((d) => {
      if (d.name === rowDim) return splashCoord.row;
      if (d.name === colDim) return splashCoord.col;
      return filters[d.name] || "";
    });

    try {
      const res = await fetch(`${API_BASE}/splash`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          coordinates: coords,
          target_value: targetVal,
        }),
      });

      if (!res.ok) {
        const errText = await res.text();
        alert(`Erreur de Splashing: ${errText}`);
      } else {
        setSplashCoord(null);
        setSplashInput("");
        await fetchPivot(); // Rafraîchir immédiatement la grille
      }
    } catch (err: any) {
      alert(`Erreur: ${err.message}`);
    }
  };

  if (loading) {
    return (
      <div className="center-screen">
        <RefreshCw className="spin" size={32} />
        <p>Connexion au moteur MOLAP Qonso...</p>
      </div>
    );
  }

  if (error && !metadata) {
    return (
      <div className="center-screen error-box">
        <h2>⚠️ Impossible de contacter le serveur Qonso</h2>
        <p>{error}</p>
        <p className="hint">
          Vérifiez que le serveur Rust tourne bien sur <code>http://127.0.0.1:3001</code>
        </p>
        <button onClick={() => fetchMetadata()} className="btn-primary">
          Réessayer
        </button>
      </div>
    );
  }

  const dimNames = metadata?.dimensions.map((d) => d.name) || [];
  const filterDims = dimNames.filter((d) => d !== rowDim && d !== colDim);

  const getDimensionIcon = (dimName: string) => {
    switch (dimName) {
      case "Period":
        return <Calendar size={16} />;
      case "Products":
        return <Package size={16} />;
      case "Channels":
        return <Store size={16} />;
      case "Measures":
        return <DollarSign size={16} />;
      default:
        return <Layers size={16} />;
    }
  };

  return (
    <div className="app-container">
      {/* Top Header */}
      <header className="header">
        <div className="brand">
          <Database className="brand-icon" size={28} />
          <div>
            <h1>QONSO MOLAP</h1>
            <div style={{ display: "flex", alignItems: "center", gap: "8px", marginTop: "4px" }}>
              <span className="badge">Cube :</span>
              <select
                value={currentCube}
                onChange={(e) => setCurrentCube(e.target.value)}
                style={{
                  background: "#1e1b4b",
                  color: "#a5b4fc",
                  border: "1px solid #4338ca",
                  borderRadius: "4px",
                  fontSize: "0.8rem",
                  padding: "2px 6px",
                  cursor: "pointer",
                }}
              >
                {metadata?.cubes_list?.map((c) => (
                  <option key={c} value={c}>
                    {c}
                  </option>
                ))}
              </select>
            </div>
          </div>
        </div>

        {/* Tab Switcher */}
        <div className="tab-switcher">
          <button
            className={`tab-btn ${activeTab === "pivot" ? "active" : ""}`}
            onClick={() => setActiveTab("pivot")}
          >
            <Sliders size={16} /> Matrice Pivot
          </button>
          <button
            className={`tab-btn ${activeTab === "sheet" ? "active" : ""}`}
            onClick={() => setActiveTab("sheet")}
          >
            <Table size={16} /> Tableur IronCalc
          </button>
        </div>

        <div className="header-actions">
          <button onClick={fetchPivot} className="btn-secondary" disabled={refreshing}>
            <RefreshCw size={16} className={refreshing ? "spin" : ""} />
            Rafraîchir
          </button>
        </div>
      </header>

      {/* Control Bar: Dimensions & Filters */}
      <div className="control-bar">
        <div className="control-group">
          <label>
            <Sliders size={14} /> Lignes :
          </label>
          <select
            value={rowDim}
            onChange={(e) => {
              const newRow = e.target.value;
              if (newRow === colDim) setColDim(rowDim);
              setRowDim(newRow);
            }}
          >
            {dimNames.map((d) => (
              <option key={d} value={d}>
                {d}
              </option>
            ))}
          </select>
        </div>

        <div className="control-group">
          <label>
            <Sliders size={14} /> Colonnes :
          </label>
          <select
            value={colDim}
            onChange={(e) => {
              const newCol = e.target.value;
              if (newCol === rowDim) setRowDim(colDim);
              setColDim(newCol);
            }}
          >
            {dimNames.map((d) => (
              <option key={d} value={d}>
                {d}
              </option>
            ))}
          </select>
        </div>

        <div className="filter-divider" />

        {/* Dynamic Filters for other dimensions */}
        {filterDims.map((fDim) => {
          const dimObj = metadata?.dimensions.find((d) => d.name === fDim);
          return (
            <div key={fDim} className="control-group">
              <label>
                {getDimensionIcon(fDim)} {fDim} :
              </label>
              <select
                value={filters[fDim] || dimObj?.elements[0]?.name}
                onChange={(e) =>
                  setFilters({ ...filters, [fDim]: e.target.value })
                }
              >
                {dimObj?.elements.map((elem) => (
                  <option key={elem.name} value={elem.name}>
                    {elem.is_consolidated ? `Σ ${elem.name}` : elem.name}
                  </option>
                ))}
              </select>
            </div>
          );
        })}
      </div>

      {/* Grid Canvas or IronCalc Sheet */}
      <main className="content">
        {activeTab === "pivot" ? (
          <>
            <div className="grid-card">
              <div className="grid-header">
                <div className="grid-title">
                  <h3>
                    Matrice Croisée Dynamique ({rowDim} × {colDim})
                  </h3>
                  <p className="grid-subtitle">
                    Filtres actifs :{" "}
                    {filterDims.map((d) => `${d} = ${filters[d] || "Tous"}`).join(" | ")}
                  </p>
                </div>
                <div className="hint-pill">
                  <Sparkles size={14} /> Cliquez sur une cellule pour tester le <strong>Splashing</strong>
                </div>
              </div>

              <div className="table-wrapper">
                <table className="molap-table">
                  <thead>
                    <tr>
                      <th className="corner-cell">
                        {rowDim} \ {colDim}
                      </th>
                      {pivotData?.col_elements.map((colName) => {
                        const colDimObj = metadata?.dimensions.find((d) => d.name === colDim);
                        const elem = colDimObj?.elements.find((e) => e.name === colName);
                        const isConsol = elem?.is_consolidated;
                        return (
                          <th
                            key={colName}
                            className={isConsol ? "header-col-consolidated" : "header-col-base"}
                          >
                            {isConsol && <span className="sigma">Σ </span>}
                            {colName}
                          </th>
                        );
                      })}
                    </tr>
                  </thead>
                  <tbody>
                    {pivotData?.rows.map((row) => {
                      const rowDimObj = metadata?.dimensions.find((d) => d.name === rowDim);
                      const isRowConsol = rowDimObj?.elements.find(
                        (e) => e.name === row.row_element
                      )?.is_consolidated;

                      return (
                        <tr
                          key={row.row_element}
                          className={isRowConsol ? "row-consolidated" : "row-base"}
                        >
                          <td className="row-header">
                            {isRowConsol && <span className="sigma">Σ </span>}
                            <strong>{row.row_element}</strong>
                          </td>
                          {row.values.map((val, idx) => {
                            const colName = pivotData.col_elements[idx];
                            const isZero = val === 0;

                            return (
                              <td
                                key={idx}
                                className={`cell-value ${isRowConsol ? "cell-consol" : ""} ${
                                  isZero ? "cell-zero" : ""
                                }`}
                                onClick={() => {
                                  setSplashCoord({
                                    row: row.row_element,
                                    col: colName,
                                    currentVal: val,
                                  });
                                  setSplashInput(val.toString());
                                }}
                                title="Cliquer pour modifier ou ventiler (Splashing)"
                              >
                                {val.toLocaleString("fr-FR", {
                                  minimumFractionDigits: 2,
                                  maximumFractionDigits: 2,
                                  })}
                              </td>
                            );
                          })}
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            </div>

            {/* Explanatory banner */}
            <div className="info-banner">
              <TrendingUp size={24} className="info-icon" />
              <div>
                <h4>Moteur MOLAP Palo en Temps Réel</h4>
                <p>
                  Toutes les lignes et colonnes préfixées par <strong>Σ</strong> sont des nœuds
                  consolidés calculés à la volée par récursion d'arbres. Vous pouvez cliquer sur
                  n'importe quel total pour fixer une valeur cible : le moteur Rust la ventilera
                  instantanément sur les sous-feuilles (Splashing).
                </p>
              </div>
            </div>
          </>
        ) : (
          <IronCalcSheet
            metadata={metadata}
            pivotData={pivotData}
            rowDim={rowDim}
            colDim={colDim}
            filters={filters}
            onRefreshMOLAP={fetchPivot}
          />
        )}
      </main>

      {/* Splashing Dialog Modal */}
      {splashCoord && (
        <div className="modal-overlay" onClick={() => setSplashCoord(null)}>
          <div className="modal-box" onClick={(e) => e.stopPropagation()}>
            <h3>✨ Write-Back & Splashing Palo</h3>
            <p>
              Modification de la coordonnée :<br />
              <strong>
                [{splashCoord.row}] × [{splashCoord.col}]
              </strong>
            </p>
            <p className="current-hint">
              Valeur actuelle :{" "}
              {splashCoord.currentVal.toLocaleString("fr-FR", {
                minimumFractionDigits: 2,
              })}
            </p>

            <div className="input-group">
              <label>Nouvelle valeur cible :</label>
              <input
                type="number"
                step="any"
                value={splashInput}
                autoFocus
                onChange={(e) => setSplashInput(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") handleApplySplash();
                }}
              />
            </div>

            <p className="modal-desc">
              Si cette cellule est un total consolidé (ex. <em>2024-Q1</em> ou{" "}
              <em>Consoles-Salon</em>), cette nouvelle valeur sera redistribuée
              proportionnellement sur l'ensemble des feuilles sous-jacentes.
            </p>

            <div className="modal-actions">
              <button onClick={() => setSplashCoord(null)} className="btn-secondary">
                Annuler
              </button>
              <button onClick={handleApplySplash} className="btn-primary">
                Appliquer le Splashing
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
