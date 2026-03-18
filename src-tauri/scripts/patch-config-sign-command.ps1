jq '.bundle.windows.signCommand = input.signCommand' `
  ../tauri.conf.json `
  ../sign.command.json `
  > tmp.json

Move-Item tmp.json ../tauri.conf.json -Force
