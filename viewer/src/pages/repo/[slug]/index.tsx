import Link from "next/link";
import { useRouter } from "next/router";

// Assuming RepoStats is an object with keys as stat names and values as stat figures
interface Repo {
  name: string;
  description: string;
  source: string;
  stats: string[];
}

interface RepoData {
  [key: string]: {
    name: string;
    description: string;
    source: string;
    stats: string[];
  };
}
interface RepoInfoProps {
  name: string;
  description: string;
  stats: string[];
}

// Simulated database
const repoData: RepoData = {
  react: {
    name: "React",
    description:
      "A declarative, efficient, and flexible JavaScript library for building user interfaces.",
    source: "github.com/facebook/react",
    stats: ["count-num-files", "grep-todos", "count-package-json-deps"],
  },
  postgres: {
    name: "Postgres",
    description:
      "Postgres is a powerful, open source object-relational database system.",
    source: "github.com/postgres/postgres",
    stats: ["count-num-files", "grep-todos", "count-typedefs"],
  },
};

export async function getServerSideProps({
  params,
}: {
  params: { slug?: string };
}) {
  const { slug } = params;
  if (!slug || !(slug in repoData)) {
    return {
      props: {
        name: "placeholder",
        description: "placeholder",
        stats: ["example"],
      },
    };
  }
  const data = repoData[slug];
  // Pass data to the page via props
  return { props: { ...data } };
}
const RepoInfo: React.FC<RepoInfoProps> = ({ name, description, stats }) => {
  const router = useRouter();
  const currentPath = router.asPath;
  return (
    <div>
      <h1>{name}</h1>
      <p>{description}</p>
      <ul>
        {stats.map((stat, index) => (
          <li key={index}>
            <Link href={`${currentPath}/stat/${stat}`}>
              {stat.replace(/-/g, " ")}
            </Link>
          </li>
        ))}
      </ul>
    </div>
  );
};

export default RepoInfo;
