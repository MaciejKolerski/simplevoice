# Zadanie 2.1: Naprawa szablonów dostawców BYOK (Anthropic, Gemini, OpenRouter)

## Opis problemu
W widoku `src/views/ModelsView.tsx` użytkownik może wybrać OpenAI, OpenRouter, Anthropic Claude oraz Google Gemini. Jednak backend w pliku `cloud.rs` (funkcja `transcribe_cloud`) przesyła plik dźwiękowy za pomocą zapytania multipart typu OpenAI (wysyłając dane na endpoint `/audio/transcriptions`). 
- Anthropic Claude nie ma takiego endpointu.
- Google Gemini wymaga Gemini File API.
Użycie tych modeli powoduje błędy 400/404.

## Pliki do modyfikacji
- [`src/views/ModelsView.tsx`](file:///home/woro/Dokumenty/simplevoice/src/views/ModelsView.tsx)
- [`src-tauri/src/stt/cloud.rs`](file:///home/woro/Dokumenty/simplevoice/src-tauri/src/stt/cloud.rs)

## Zalecane rozwiązanie
1. Opcja A: Usuń nieobsługiwanych dostawców z listy wyboru w interfejsie użytkownika.
2. Opcja B (rekomendowana): Zaimplementuj dedykowaną obsługę API w `cloud.rs` dla poszczególnych dostawców (np. wysyłanie odpowiedniego payloadu dla Gemini File API lub wyłączenie opcji transkrypcji dla modeli tylko-tekstowych).

## Lista kroków do wykonania
- [ ] Przegląd endpointów i formatów danych akceptowanych przez Anthropic i Google Gemini.
- [ ] Dostosowanie logiki `transcribe_cloud` w Rust do wysyłania poprawnych żądań HTTP w zależności od wybranego dostawcy.
- [ ] Ukrycie lub zablokowanie niedostępnych opcji w widoku konfiguracji modeli w React.
- [ ] Przetestowanie połączeń i transkrypcji z wybranymi dostawcami BYOK.
