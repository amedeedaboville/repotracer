import { type Session } from "next-auth";
import { SessionProvider } from "next-auth/react";
import { type AppType } from "next/app";
import { Layout } from "~/components/layout";
import { ChakraProvider } from "@chakra-ui/react";
import { api } from "~/utils/api";

import "~/styles/globals.css";

import { Inter } from "next/font/google";

// If loading a variable font, you don't need to specify the font weight
const inter = Inter({ subsets: ["latin"] });

const MyApp: AppType<{ session: Session | null }> = ({
  Component,
  pageProps: { session, ...pageProps },
}) => {
  return (
    <>
      <style jsx global>{`
        html {
          font-family: ${inter.style.fontFamily};
        }
      `}</style>
      <ChakraProvider>
        <Layout>
          <SessionProvider session={session}>
            <Component {...pageProps} />
          </SessionProvider>
        </Layout>
      </ChakraProvider>
    </>
  );
};

export default api.withTRPC(MyApp);
