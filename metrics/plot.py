import pandas as pd
import sys
import math
from matplotlib import pyplot as plt
from matplotlib.ticker import FuncFormatter
from itertools import cycle

if len(sys.argv) < 2:
    print("Usage: python plot_csv.py file1.csv file2.csv ...")
    sys.exit(1)

csv_files = sys.argv[1:]
dfs = [pd.read_csv(f) for f in csv_files]

n = len(dfs[0].columns) - 1  # number of metrics (skip timestamp)
cols = 2
rows = math.ceil(n / cols)

fig, axes = plt.subplots(rows, cols, figsize=(6*cols, 4*rows))
axes = axes.flatten()

# human-readable formatter
def human_format(x, pos):
    if x >= 1e9: return f'{x/1e9:.1f}G'
    elif x >= 1e6: return f'{x/1e6:.1f}M'
    elif x >= 1e3: return f'{x/1e3:.1f}K'
    else: return f'{x:.0f}'

# colors cycle
colors = cycle(plt.rcParams['axes.prop_cycle'].by_key()['color'])

for i, col in enumerate(dfs[0].columns[1:]):
    color_cycle = cycle(plt.rcParams['axes.prop_cycle'].by_key()['color'])
    for df, file in zip(dfs, csv_files):
        x_values = range(len(df))  # each CSV has its own x-axis
        axes[i].plot(x_values, df[col], label=file, color=next(color_cycle))
    axes[i].set_title(col)
    axes[i].set_xlabel("Time (0→end)")
    axes[i].set_ylabel(col)
    axes[i].yaxis.set_major_formatter(FuncFormatter(human_format))
    axes[i].grid(True)
    axes[i].legend(fontsize=8)

# hide extra subplots
for j in range(i+1, len(axes)):
    fig.delaxes(axes[j])

plt.tight_layout()
plt.show()
