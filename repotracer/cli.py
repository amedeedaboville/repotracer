from argparse import ArgumentParser
from commands.run import define_command as configure_run


def main():
    parser = ArgumentParser(
        prog="rt", description="Collect statistics about git repos over time"
    )
    subparser = parser.add_subparsers(dest="command", required=True)

    run_parser = subparser.add_parser("run", help="Collect one or more stats")
    configure_run(run_parser)
    args = parser.parse_args()

    args.func(args)


if __name__ == "__main__":
    main()
