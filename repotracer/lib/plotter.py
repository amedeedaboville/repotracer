import matplotlib
import matplotlib.pyplot as plt
import seaborn as sns


def rotate(l, n):
    return l[n:] + l[:n]


sns.set_theme()
sns.set_style("whitegrid")
sns.set_palette(rotate(sns.color_palette("deep"), 6))


def plot(repo_name, stat_name, stat_description, df):
    plt.rcParams["figure.dpi"] = 140
    plt.rcParams["figure.figsize"] = (12.8, 9.6)
    image_path = f"./stats/{repo_name}/{stat_name}.png"
    ax = df.plot()
    last_date = df.index.values[-1]
    last_value = df.iloc[-1].total
    ax.annotate(last_value, (last_date, last_value))
    plt.xlabel("Date")
    plt.title(f"repotracer for {repo_name}: {stat_name}")
    plt.suptitle(stat_description, y=0.94, size="x-large", weight="semibold")
    plt.savefig(image_path, bbox_inches="tight")
