# QONSO MOLAP & Spreadsheet Platform

Plateforme SaaS de planification et modélisation multidimensionnelle (MOLAP) haute performance développée en Rust et connectée au moteur de tableur WebAssembly IronCalc.

## Architecture

- **Backend Rust (Axum)** : Moteur MOLAP In-Memory avec calculs récursifs d'arbres, moteur de règles métiers (Rules Engine), support multi-cubes et persistance journalisée (WAL).
- **Frontend React 19 (TypeScript + Vite)** : Navigation par matrice pivot dynamique et intégration du tableur IronCalc (WASM) avec formules de cube interactives (`=QONSO.VALUE(...)`).
- **Déploiement Docker & Traefik** : Prêt pour la production derrière le reverse proxy Traefik avec terminaison TLS / HTTPS sur `qonso.tensordecision.com`.

## Déploiement en Production (Serveur 147.79.100.70)

1. Cloner le dépôt sur le serveur :
   ```bash
   git clone git@github.com:usikbyr/qonso.git
   cd qonso
   ```

2. Assurez-vous que le réseau Docker Traefik existe :
   ```bash
   docker network ls | grep traefik-public || docker network create traefik-public
   ```

3. Lancer l'application :
   ```bash
   docker compose up -d --build
   ```

4. L'application est alors automatiquement accessible sur :
   - `https://qonso.tensordecision.com` (Frontend + API `/api/*`)
