# Zadanie 3.3: Nadpisywanie globalnego obiektu localStorage

## Opis problemu
W pliku `src/main.tsx` nadpisano natywne metody `localStorage.setItem` oraz `localStorage.removeItem`, aby automatycznie synchronizować zmiany konfiguracji z plikiem konfiguracyjnym w backendzie (za pomocą `save_config`). Nadpisywanie globalnych obiektów JS jest antywzorcem i może powodować nieprzewidziane błędy.

## Pliki do modyfikacji
- [`src/main.tsx`](file:///home/woro/Dokumenty/simplevoice/src/main.tsx)
- [`src/context/ConfigContext.tsx`](file:///home/woro/Dokumenty/simplevoice/src/context/ConfigContext.tsx) (lub nowy plik)

## Zalecane rozwiązanie
1. Usuń monkey-patching z `main.tsx`.
2. Stwórz niestandardowy provider stanu konfiguracji w React (np. `ConfigProvider`) lub dedykowany hook (np. `usePersistedConfig`).
3. Zarządzaj zapisem i odczytem konfiguracji w sposób jawny, wywołując komendy Tauri wewnątrz tego hooka/providera przy każdej modyfikacji ustawień w UI.

## Lista kroków do wykonania
- [ ] Usunięcie bloku kodu nadpisującego `localStorage` w `main.tsx`.
- [ ] Utworzenie `ConfigProvider` w React zarządzającego stanem konfiguracji aplikacji.
- [ ] Zintegrowanie zapisu stanu z komendami Tauri (np. `save_config`).
- [ ] Refaktoryzacja komponentów korzystających z ustawień do używania nowego providera / hooka zamiast bezpośredniego zapisu do `localStorage`.
- [ ] Testy zachowania ustawień po ponownym uruchomieniu aplikacji.
