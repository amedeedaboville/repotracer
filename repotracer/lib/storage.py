import pandas as pd
import os

from .config import get_stat_storage_config

import logging

logger = logging.getLogger(__name__)
logger.setLevel(logging.INFO)
logger.addHandler(logging.StreamHandler())


class Storage(object):
    def __init__(self):
        pass

    def save(self, df: pd.DataFrame):
        pass

    def load(self) -> pd.DataFrame:
        pass


class CsvStorage(Storage):
    def __init__(self):
        self.config = get_stat_storage_config()

    def build_path(self, repo_name, stat_name):
        return self.config["path"] + f"/{repo_name}/{stat_name}.csv"

    def load(self, repo_name, stat_name) -> pd.DataFrame | None:
        path = self.build_path(repo_name, stat_name)
        logger.debug(f"{os.getcwd()}: Loading {stat_name} from {repo_name} from {path}")
        try:
            df = pd.read_csv(path, index_col=0)
            df.index = pd.to_datetime(df.index)
            return df
        except FileNotFoundError:
            logger.debug("df not found")
            return None

    def save(self, repo_name, stat_name, df: pd.DataFrame):
        path = self.build_path(repo_name, stat_name)
        # if the path doesn't exist, create it
        if not os.path.exists(os.path.dirname(path)):
            os.makedirs(os.path.dirname(path), exist_ok=True)
        logger.info(f"Saving {stat_name} to {path}")
        df.to_csv(path, date_format="%Y-%m-%d")
