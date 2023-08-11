import matplotlib
import matplotlib.pyplot as plt


def plot(repo_name, stat_name, stat_description, df):
    plt.rcParams["figure.dpi"] = 140
    plt.rcParams["figure.figsize"] = (12.8, 9.6)
    image_path = f"./stats/{repo_name}/{stat_name}.png"
    df.plot()
    plt.title(f"{repo_name} - {stat_name}")
    plt.suptitle(stat_description)
    plt.savefig(image_path, bbox_inches="tight")
