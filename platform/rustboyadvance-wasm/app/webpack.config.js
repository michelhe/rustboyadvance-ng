const CopyWebpackPlugin = require("copy-webpack-plugin");
const FaviconsWebpackPlugin = require('favicons-webpack-plugin')

const path = require('path');

module.exports = {
  entry: "./bootstrap.js",
  output: {
    path: path.resolve(__dirname, "dist"),
    filename: "bootstrap.js",
  },
  mode: "development",
  plugins: [
    new CopyWebpackPlugin(['index.html', '../../../assets/icon.png']),
    new FaviconsWebpackPlugin({
      logo: '../../../assets/icon.png',
      cache: true,
      mode: 'webapp',
      devMode: 'webapp',
      favicons: {
        appName: 'rustboyadvance',
        appDescription: 'Web Demo for rustboyadvance',
        developerName: 'michel',
        developerURL: null, // prevent retrieving from the nearest package.json
        background: '#ddd',
        theme_color: '#333',
        icons: {
          coast: false,
          yandex: false,
        }
      }
    })
  ],
};
