import { useEffect } from "react";
import { BrowserRouter, Route, Routes, useLocation } from "react-router-dom";
import { AuthProvider } from "./lib/auth";
import { Header } from "./components/Header";
import { Footer } from "./components/Footer";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { SearchPage } from "./pages/SearchPage";
import { ShowPage } from "./pages/ShowPage";
import { SkipGuidePage } from "./pages/SkipGuidePage";
import { GuideDetailPage } from "./pages/GuideDetailPage";
import { GuideEditorPage } from "./pages/GuideEditorPage";
import { LoginPage } from "./pages/LoginPage";
import { AccountPage } from "./pages/AccountPage";
import { AboutPage } from "./pages/AboutPage";
import { SupportPage } from "./pages/SupportPage";
import { PrivacyPage } from "./pages/PrivacyPage";
import { TermsPage } from "./pages/TermsPage";
import { NotFoundPage } from "./pages/NotFoundPage";

/** Reset scroll on page navigation — an SPA otherwise keeps the previous
 *  page's scroll position. Keyed on pathname only, so same-page query-param
 *  changes (search, guide mode) don't jump the view. */
function ScrollToTop() {
  const { pathname } = useLocation();
  useEffect(() => {
    window.scrollTo(0, 0);
  }, [pathname]);
  return null;
}

export default function App() {
  return (
    <BrowserRouter>
      <ScrollToTop />
      <AuthProvider>
        <div className="flex min-h-screen flex-col bg-zinc-950 text-zinc-100">
          <Header />
          <main className="flex-1">
            <ErrorBoundary>
              <Routes>
                <Route path="/" element={<SearchPage />} />
                <Route path="/login" element={<LoginPage />} />
                <Route path="/account" element={<AccountPage />} />
                <Route path="/about" element={<AboutPage />} />
                <Route path="/support" element={<SupportPage />} />
                <Route path="/privacy" element={<PrivacyPage />} />
                <Route path="/terms" element={<TermsPage />} />
                <Route path="/shows/:id" element={<ShowPage />} />
                <Route path="/shows/:id/skip-guide" element={<SkipGuidePage />} />
                <Route path="/shows/:id/guides/new" element={<GuideEditorPage />} />
                <Route path="/shows/:id/guides/:guideId/edit" element={<GuideEditorPage />} />
                <Route path="/shows/:id/guides/:guideId" element={<GuideDetailPage />} />
                <Route path="*" element={<NotFoundPage />} />
              </Routes>
            </ErrorBoundary>
          </main>
          <Footer />
        </div>
      </AuthProvider>
    </BrowserRouter>
  );
}
