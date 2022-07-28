#include <QtWebKitWidgets/QWebView>
#include <QtWebKitWidgets/QWebFrame>
#include <QUrl>

extern "C" void qwebview_load(QWebView *view, char *url) {
  view->load(QUrl(url));
}

extern "C" void qwebview_eval(QWebView *view, char *code) {
  view->page()->mainFrame()->evaluateJavaScript(QString(code));
}
