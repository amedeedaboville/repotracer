import matplotlib
import matplotlib.pyplot as plt
from matplotlib.dates import MonthLocator, ConciseDateFormatter, AutoDateFormatter
import seaborn as sns


def rotate(l, n):
    return l[n:] + l[:n]


sns.set_theme()
sns.set_style("whitegrid")
sns.set_palette(rotate(sns.color_palette("deep"), 2))


def plot(repo_name, stat_name, stat_description, df, run_at):
    plt.rcParams["figure.dpi"] = 140
    plt.rcParams["figure.figsize"] = (12.8, 9.6)
    image_path = f"./stats/{repo_name}/{stat_name}.png"
    ax = df.plot()

    last_date = df.index.values[-1]
    last_value = df.iloc[-1].total
    ax.annotate(last_value, (last_date, last_value))

    plt.xlabel("Date")
    plt.suptitle(stat_description, y=0.93, size="xx-large", weight="semibold")
    plt.title(
        f"{repo_name}:{stat_name} by repotracer at {run_at.strftime('%Y-%m-%d %H:%M:%S')}",
        loc="right",
        fontsize="small",
        weight="light",
        alpha=0.5,
    )
    # ax.xaxis.set_major_locator(MonthLocator())
    # todo set a formatter that displays either single letter month or full month name
    # ax.xaxis.set_major_formatter(ConciseDateFormatter(MonthLocator()))

    plt.savefig(image_path, bbox_inches="tight")
