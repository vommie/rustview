#!/bin/bash
# usage: seed_huge.sh COPIES -> lib_huge/huge_<COPIES*1000> made of hardlinks of images1000, seeded into home_seed_huge (records + thumbnails)
P=${PERF_DIR:?set PERF_DIR to the scratch directory}
N=${1:-100}
D=$P/lib_huge/huge_$((N*1000))
mkdir -p $D
for c in $(seq 1 $N); do for f in $P/images1000/*.webp; do ln -f $f $D/c${c}_$(basename $f); done; done
H=$P/home_seed_huge
rm -rf $H; mkdir -p $H/.config/rustview
sed -e "s#\"root_folder\": \"[^\"]*\"#\"root_folder\": \"$P/lib_huge\"#" -e "s#\"last_folder_path\": \"[^\"]*\"#\"last_folder_path\": \"$D\"#" $P/home_seed/.config/rustview/settings.json > $H/.config/rustview/settings.json
HOME=$H XDG_CONFIG_HOME=$H/.config XDG_DATA_HOME=$H/.local/share ${PERF_SEED_BIN:-target/debug/examples/perf_seed} $H 200 $P/lib_huge $D
