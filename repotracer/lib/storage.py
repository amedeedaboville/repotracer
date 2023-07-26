import pandas as pd

class Storage(object):
    def __init__(self, config):
        pass

    def save(self, df: pd.DataFrame):
        pass

    def load(self) -> pd.DataFrame:
        pass


class CSVStorage(Storage):
    def __init__(self, config):
        self.path = path
        self.df = pd.read_csv(path, index_col=0)
        self.df.index = pd.to_datetime(self.df.index)

    def save(self):
        self.df.to_csv(self.path)
