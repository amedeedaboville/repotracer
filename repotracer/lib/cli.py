from argparse import ArgumentParser


def main():
    parser = ArgumentParser(
        prog="rt", description="Collect statistics about git repos over time"
    )
    subparser = parser.add_subparsers(dest="command")
    run = subparser.add_parser("run", help="Collect one or more stats")
    run.add_argument("name", help="Name of the stat to collect")
    args = parser.parse_args()

    if args.command == "run":
        print(f"Running stat {args.name}")
    else:
        parser.print_help()


if __name__ == "__main__":
    main()
