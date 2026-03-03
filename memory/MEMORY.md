# Clodo Hotel — Memory

## App Standalone (standalone/)

Architecture : Node.js Express + WebSocket server + même React webview.
- Démarrage : `cd standalone && npm install && npx tsx server.ts` (depuis la racine du projet buildé)
- Pré-requis : `npm run build` depuis la racine pour générer `dist/assets/` et `dist/webview/`
- Accès : http://localhost:3000
- Auto-détecte les sessions Claude actives des dernière 1h au démarrage
- Bouton "+ Agent" ouvre une modale `SessionPickerModal` avec les sessions disponibles

Fichiers clés standalone :
- `standalone/server.ts` — Entrée Express + WebSocket
- `standalone/src/sessionScanner.ts` — Scan ~/.claude/projects/ (lit le champ `cwd` du JSONL)
- `standalone/src/agentServer.ts` — Lifecycle agents sans VS Code
- `webview-ui/src/vscodeApi.ts` — Runtime detection VS Code vs WebSocket (modifié)
- `webview-ui/src/components/SessionPickerModal.tsx` — Modale de sélection de session

Différences vs extension VS Code :
- Pas de `vscode.Terminal` → `sessionId` + `projectDir` seulement
- Persistence dans `~/.clodo-hotel/standalone-state.json` au lieu de workspaceState
- Message `availableSessions` nouveau (projets/sessions détectés)
- Layout partagé avec l'extension : `~/.clodo-hotel/layout.json`



## Standalone — Bugs corrigés

- **`existingAgents` doit arriver AVANT `layoutLoaded`** : la webview buffe les agents dans `pendingAgents` et les flush dans le handler `layoutLoaded`. Si `layoutLoaded` arrive en premier, `pendingAgents` est vide → aucun personnage.
- **`removeAgent` doit aussi nettoyer `knownJsonlFiles`** : sinon `scanSessions` marque la session comme `isTracked` après suppression et elle n'apparaît plus dans le picker.
- **`restoreAgents` ne doit s'appeler que si `agents.size === 0`** : évite les doublons de watchers au refresh.
- **`isSessionPickerOpen`** : séparer l'état d'ouverture de la modale des données `availableSessions`, sinon tout broadcast `availableSessions` ouvre la modale.
- **`extractProjectPath` doit scanner plusieurs lignes** : le premier record JSONL est souvent un `file-history-snapshot` sans `cwd`. Scanner jusqu'à 8KB.
- **Fallback dir hash → path** : `dirName.replace(/-/g, '/')` casse les noms avec tirets (ex: `mon-projet` → `mon/projet`). Utiliser `path.basename(projectPath)` après extraction correcte du `cwd`.

## Setup / Debug

### Pas d'avatar qui spawn → ouvrir un dossier dans le Dev Host
Symptôme : terminal s'ouvre, Claude se lance, mais aucun personnage n'apparaît dans le bureau pixel art.
Cause : `getProjectDirPath()` retourne `null` si aucun dossier n'est ouvert dans la fenêtre Extension Dev Host → `launchNewTerminal()` fait un early return avant d'envoyer `agentCreated`.
Fix : dans la fenêtre `[Extension Development Host]`, faire File → Open Folder sur n'importe quel projet.
Fichier concerné : `src/agentManager.ts` ligne ~50.
