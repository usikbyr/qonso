import React, { useEffect, useRef, useState } from "react";
import { IronCalc, init, Model, type IronCalcHandle } from "@ironcalc/workbook";
import "@ironcalc/workbook/style.css";
import { RefreshCw, ArrowLeftRight } from "lucide-react";

interface DimensionDto {
  name: string;
  elements: { name: string; is_consolidated: boolean; children: string[] }[];
}

interface MetadataResponse {
  cube_name: string;
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

interface IronCalcSheetProps {
  metadata: MetadataResponse | null;
  pivotData: PivotGridResponse | null;
  rowDim: string;
  colDim: string;
  filters: Record<string, string>;
  onRefreshMOLAP: () => void;
}

export const IronCalcSheet: React.FC<IronCalcSheetProps> = ({
  metadata,
  pivotData,
  rowDim,
  colDim,
  filters,
  onRefreshMOLAP,
}) => {
  const ironCalcRef = useRef<IronCalcHandle>(null);
  const [model, setModel] = useState<Model | null>(null);
  const [wasmReady, setWasmReady] = useState(false);
  const [syncStatus, setSyncStatus] = useState<string>("Prêt");

  // Initialisation de WASM IronCalc
  useEffect(() => {
    let active = true;
    async function setupWasm() {
      try {
        await init();
        if (active) {
          setWasmReady(true);
        }
      } catch (err) {
        console.error("Erreur lors du chargement de IronCalc WASM:", err);
      }
    }
    setupWasm();
    return () => {
      active = false;
    };
  }, []);

  // Création du modèle de classeur et injection de la vue pivot MOLAP
  useEffect(() => {
    if (!wasmReady) return;

    try {
      // Model(name, locale, timezone, language)
      const newModel = new Model("Qonso Workbook", "fr", "Europe/Paris", "fr");

      // Remplissage avec les données de la matrice pivot si disponible
      if (pivotData && metadata) {
        const sheetIdx = 0; // Feuille par défaut

        // Titre du rapport
        newModel.setUserInput(sheetIdx, 1, 1, `Qonso MOLAP - ${metadata.cube_name}`);
        newModel.setUserInput(
          sheetIdx,
          2,
          1,
          `Filtres: ${Object.entries(filters)
            .map(([k, v]) => `${k}=${v}`)
            .join(", ")}`
        );

        // En-têtes de colonnes
        newModel.setUserInput(sheetIdx, 4, 1, `${rowDim} \\ ${colDim}`);
        pivotData.col_elements.forEach((colName, cIdx) => {
          newModel.setUserInput(sheetIdx, 4, cIdx + 2, colName);
        });

        // Lignes et valeurs
        pivotData.rows.forEach((row, rIdx) => {
          const rowNum = rIdx + 5;
          newModel.setUserInput(sheetIdx, rowNum, 1, row.row_element);

          row.values.forEach((val, cIdx) => {
            const colNum = cIdx + 2;
            newModel.setUserInput(sheetIdx, rowNum, colNum, val.toString());
          });
        });
      }

      setModel(newModel);
      setSyncStatus("Données MOLAP injectées avec succès");
    } catch (err) {
      console.error("Erreur de population du modèle IronCalc:", err);
      setSyncStatus("Erreur d'initialisation du tableur");
    }
  }, [wasmReady, pivotData, metadata, rowDim, colDim, filters]);

  // Synchroniser à nouveau les données MOLAP dans le tableur
  const handleReloadFromMOLAP = () => {
    onRefreshMOLAP();
    setSyncStatus("Rechargement depuis le cube MOLAP...");
  };

  // Résolution d'une formule QONSO.VALUE(coord1, coord2, ...)
  const [customFormulaInput, setCustomFormulaInput] = useState<string>(
    '=QONSO.VALUE("2024-Q1", "NES", "Boutique-Paris", "Revenue")'
  );
  const [evaluating, setEvaluating] = useState<boolean>(false);
  const [targetCell, setTargetCell] = useState<string>("B16");

  // Write-Back / Splashing depuis le tableur
  const [splashInputCoord, setSplashInputCoord] = useState<string>(
    "2024-Q1, NES, Boutique-Paris, Revenue"
  );
  const [splashInputValue, setSplashInputValue] = useState<string>("5000");

  const handleEvaluateCustomFormula = async () => {
    if (!model) return;
    setEvaluating(true);
    setSyncStatus("Évaluation de la formule de Cube...");

    try {
      // Extraction des arguments dans QONSO.VALUE(...) ou QONSO(...)
      const match = customFormulaInput.match(/=?(?:QONSO\.VALUE|QONSO)\s*\((.*)\)/i);
      if (!match) {
        throw new Error("Syntaxe attendue : =QONSO.VALUE(\"coord1\", \"coord2\", ...)");
      }

      // Parser les arguments (en nettoyant les guillemets et espaces)
      const rawArgs = match[1];
      const coords = rawArgs
        .split(",")
        .map((s) => s.trim().replace(/^["']|["']$/g, ""));

      const apiBase = import.meta.env.VITE_API_URL || "/api";
      const res = await fetch(`${apiBase}/cell`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ coordinates: coords }),
      });

      if (!res.ok) {
        const text = await res.text();
        throw new Error(text || "Erreur de résolution MOLAP");
      }

      const data = await res.json();
      const val = data.value;

      // Injection dans la cellule cible
      const colLetter = targetCell.charAt(0).toUpperCase();
      const colNum = colLetter.charCodeAt(0) - 64; // 'A' = 1, 'B' = 2, etc.
      const rowNum = parseInt(targetCell.substring(1), 10) || 16;

      const sheetIdx = 0;
      model.setUserInput(sheetIdx, rowNum, colNum, val.toString());
      model.setUserInput(sheetIdx, rowNum, 1, `QONSO.VALUE`);
      model.evaluate();

      setSyncStatus(`Résolu: ${val.toLocaleString("fr-FR")} injecté en ${targetCell}`);
    } catch (err: any) {
      setSyncStatus(`Erreur formule: ${err.message}`);
      alert(err.message);
    } finally {
      setEvaluating(false);
    }
  };

  const handleExecuteSplashFromSheet = async () => {
    setSyncStatus("Envoi du Splashing au serveur MOLAP...");
    try {
      const coords = splashInputCoord.split(",").map((s) => s.trim());
      const targetVal = parseFloat(splashInputValue);
      if (isNaN(targetVal)) throw new Error("Valeur invalide");

      const apiBase = import.meta.env.VITE_API_URL || "/api";
      const res = await fetch(`${apiBase}/splash`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          coordinates: coords,
          target_value: targetVal,
        }),
      });

      if (!res.ok) {
        const errText = await res.text();
        throw new Error(errText);
      }

      setSyncStatus(`Splashing appliqué avec succès !`);
      onRefreshMOLAP();
    } catch (err: any) {
      setSyncStatus(`Erreur Splashing: ${err.message}`);
      alert(err.message);
    }
  };

  return (
    <div className="ironcalc-container">
      {/* Barre de formule de Cube & Splashing */}
      <div className="ironcalc-formula-bar">
        <div className="formula-box">
          <span className="fx-label">fx</span>
          <input
            type="text"
            className="formula-input"
            value={customFormulaInput}
            onChange={(e) => setCustomFormulaInput(e.target.value)}
            placeholder='=QONSO.VALUE("2024-Q1", "NES", "Boutique-Paris", "Revenue")'
          />
          <span className="dest-label">vers cellule</span>
          <input
            type="text"
            className="target-cell-input"
            value={targetCell}
            onChange={(e) => setTargetCell(e.target.value)}
            style={{ width: "60px", textAlign: "center" }}
          />
          <button
            onClick={handleEvaluateCustomFormula}
            className="btn-primary formula-btn"
            disabled={evaluating}
          >
            Évaluer Formule
          </button>
        </div>

        <div className="splash-box">
          <span className="splash-label">Write-Back / Splash :</span>
          <input
            type="text"
            className="splash-coords"
            value={splashInputCoord}
            onChange={(e) => setSplashInputCoord(e.target.value)}
            placeholder="Period, Product, Channel, Measure"
            title="Coordonnées séparées par des virgules"
          />
          <input
            type="number"
            className="splash-val"
            value={splashInputValue}
            onChange={(e) => setSplashInputValue(e.target.value)}
            style={{ width: "90px" }}
          />
          <button
            onClick={handleExecuteSplashFromSheet}
            className="btn-secondary splash-btn"
          >
            Ventiler
          </button>
        </div>
      </div>

      <div className="ironcalc-toolbar">
        <div className="ironcalc-info">
          <span className="ironcalc-badge">IronCalc WASM v0.9.0</span>
          <span className="ironcalc-status">{syncStatus}</span>
        </div>
        <div className="ironcalc-actions">
          <button
            onClick={handleReloadFromMOLAP}
            className="btn-secondary"
            title="Réinjecter les données courantes du Cube MOLAP dans le tableur"
          >
            <ArrowLeftRight size={15} />
            Recharger données Cube
          </button>
        </div>
      </div>

      <div className="ironcalc-wrapper">
        {model ? (
          <IronCalc model={model} ref={ironCalcRef} />
        ) : (
          <div className="center-screen">
            <RefreshCw className="spin" size={28} />
            <p>Chargement du moteur IronCalc WebAssembly...</p>
          </div>
        )}
      </div>
    </div>
  );
};
