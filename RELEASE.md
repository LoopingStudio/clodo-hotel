# Comment faire une release

## Pré-requis (une seule fois)

### 1. Générer la clé de signature

```sh
cd tauri-app
npm run tauri signer generate -w ~/.tauri/clodo-hotel.key
```

La commande affiche une **clé publique** (commence par `dW50cnVzdGVkI...`). Garde-la.

### 2. Mettre la clé publique dans `tauri.conf.json`

Dans `tauri-app/src-tauri/tauri.conf.json`, remplace `REPLACE_WITH_YOUR_PUBLIC_KEY` :

```json
"plugins": {
  "updater": {
    "pubkey": "COLLE_TA_CLÉ_PUBLIQUE_ICI",
    ...
  }
}
```

### 3. Ajouter les secrets GitHub

Sur `github.com/pablodelucca/clodo-hotel` → **Settings → Secrets and variables → Actions** → New secret :

| Nom | Valeur |
|-----|--------|
| `TAURI_SIGNING_PRIVATE_KEY` | contenu du fichier `~/.tauri/clodo-hotel.key` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | mot de passe choisi (laisser vide si aucun) |

---

## Faire une release

### 1. Bump la version

Dans `tauri-app/src-tauri/tauri.conf.json` :
```json
"version": "0.2.0"
```

Dans `tauri-app/src-tauri/Cargo.toml` :
```toml
version = "0.2.0"
```

### 2. Commit + tag + push

```sh
git add -A
git commit -m "chore: bump version to 0.2.0"
git tag v0.2.0
git push origin main --tags
```

### 3. GitHub Actions fait le reste

- Build pour macOS, Windows, Linux
- Crée la GitHub Release avec les installeurs (`.dmg`, `.msi`, `.AppImage`)
- Génère `latest.json` pour les auto-updates

Lien de téléchargement : `https://github.com/pablodelucca/clodo-hotel/releases/latest`

---

## Ce que voient les utilisateurs

Les utilisateurs qui ont déjà l'app reçoivent automatiquement une notification dans l'app au prochain lancement. Ils cliquent **Redémarrer** et c'est à jour.
