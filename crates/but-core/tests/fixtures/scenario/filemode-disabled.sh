#!/usr/bin/env bash

git init
git config core.fileMode false
printf 'before\n' >script.sh
printf 'before\n' >added.sh
printf 'before\n' >removed.sh
printf 'before\n' >mode-only.sh
git add .
git update-index --chmod=+x script.sh
git update-index --chmod=+x removed.sh
git commit -m executable
git update-index --chmod=+x added.sh mode-only.sh
git update-index --chmod=-x removed.sh
printf 'after\n' >script.sh
printf 'after\n' >added.sh
printf 'after\n' >removed.sh
