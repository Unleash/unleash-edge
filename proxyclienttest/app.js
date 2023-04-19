let config = {
  url: "http://localhost:3063/api/frontend",
  clientKey: "*:development.38bd47b093eb2ff2f1467276c3ce7ee58407f982138c77cb9bc7ad4e",
  refreshInterval: 5,
  appName: 'myExample'
};

let client = new unleash.UnleashClient(config);
client.updateContext({ userId: "123", properties: { companyId: 'bricks' }});client.start();

console.log(client.isEnabled('custom.constraint'));

setInterval(() => {
  console.log(client.isEnabled('custom.constraint'));
}, 1000);
