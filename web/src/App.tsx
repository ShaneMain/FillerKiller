import { BrowserRouter, Route, Routes } from "react-router-dom";
import { AuthProvider } from "./lib/auth";
import { Header } from "./components/Header";
import { SearchPage } from "./pages/SearchPage";
import { ShowPage } from "./pages/ShowPage";
import { SkipGuidePage } from "./pages/SkipGuidePage";

export default function App() {
  return (
    <BrowserRouter>
      <AuthProvider>
        <div className="min-h-screen bg-zinc-950 text-zinc-100">
          <Header />
          <Routes>
            <Route path="/" element={<SearchPage />} />
            <Route path="/shows/:id" element={<ShowPage />} />
            <Route path="/shows/:id/skip-guide" element={<SkipGuidePage />} />
          </Routes>
        </div>
      </AuthProvider>
    </BrowserRouter>
  );
}
