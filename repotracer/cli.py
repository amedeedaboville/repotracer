from argparse import ArgumentParser
from commands.run import run, run_template


def main():
    parser = ArgumentParser(
        prog="rt", description="Collect statistics about git repos over time"
    )
    subparser = parser.add_subparsers(dest="command", required=True)

    run_parser = subparser.add_parser("run", help="Collect one or more stats")
    run_parser.add_argument("name", help="Name of the stat to collect")
    run_parser.set_defaults(func=run)
    # pass everything in run_template to the parser
    for arg in run_template["args"]:
        run_parser.add_argument(arg["name"], help=arg["help"], type=arg["type"])

    args = parser.parse_args()
    print(args)
    print(args.func)

    args.func(args)
    if args.command == "run":
        print(f"Running stat {args.name}")
    else:
        parser.print_help()


if __name__ == "__main__":
    main()
