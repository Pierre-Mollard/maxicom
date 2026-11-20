#!/bin/sh
# Downloads the Unicode data files the generator reads into target/ucd/<version>
set -e
cd "$(dirname "$0")/../.."
for v in 15.1.0 18.0.0; do
    mkdir -p "target/ucd/$v"
    for f in EastAsianWidth.txt PropList.txt extracted/DerivedGeneralCategory.txt; do
        curl -sSf -o "target/ucd/$v/$(basename "$f")" "https://www.unicode.org/Public/$v/ucd/$f"
    done
done
# the names, for --list
curl -sSf -o target/ucd/18.0.0/UnicodeData.txt https://www.unicode.org/Public/18.0.0/ucd/UnicodeData.txt
