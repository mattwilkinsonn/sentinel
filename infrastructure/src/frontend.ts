import * as aws from "@pulumi/aws";
import * as cloudflare from "@pulumi/cloudflare";
import { apiDomain, zireaelZoneId } from "./config";
import { apiGateway } from "./network";

// ---------------------------------------------------------------------------
// ACM Certificate for API domain (api.sentinel.zireael.dev)
// ---------------------------------------------------------------------------
export const apiCert = new aws.acm.Certificate("sentinel-api-cert", {
  domainName: apiDomain,
  validationMethod: "DNS",
});

const apiCertVal = apiCert.domainValidationOptions.apply((opts) => opts[0]);
// Strip trailing dots — ACM returns FQDNs with dots, Cloudflare rejects them
const apiCertValRecord = new cloudflare.DnsRecord(
  "sentinel-api-cert-validation",
  {
    zoneId: zireaelZoneId,
    name: apiCertVal.apply((v) => v.resourceRecordName.replace(/\.$/, "")),
    type: apiCertVal.apply((v) => v.resourceRecordType),
    content: apiCertVal.apply((v) => v.resourceRecordValue.replace(/\.$/, "")),
    ttl: 60,
  },
);

const apiCertValidated = new aws.acm.CertificateValidation(
  "sentinel-api-cert-validated",
  {
    certificateArn: apiCert.arn,
    validationRecordFqdns: [
      apiCertVal.apply((v) => v.resourceRecordName.replace(/\.$/, "")),
    ],
  },
  { dependsOn: [apiCertValRecord] },
);

// ---------------------------------------------------------------------------
// API Gateway Custom Domain
// ---------------------------------------------------------------------------
export const apiDomainName = new aws.apigatewayv2.DomainName(
  "sentinel-api-domain",
  {
    domainName: apiDomain,
    domainNameConfiguration: {
      certificateArn: apiCertValidated.certificateArn,
      endpointType: "REGIONAL",
      securityPolicy: "TLS_1_2",
    },
  },
);

new aws.apigatewayv2.ApiMapping("sentinel-api-mapping", {
  apiId: apiGateway.id,
  domainName: apiDomainName.id,
  stage: "$default",
});

// ---------------------------------------------------------------------------
// Cloudflare DNS — API domain → API Gateway
// ---------------------------------------------------------------------------
new cloudflare.DnsRecord("sentinel-api-dns", {
  zoneId: zireaelZoneId,
  name: apiDomain,
  type: "CNAME",
  content: apiDomainName.domainNameConfiguration.apply(
    (c) => c.targetDomainName,
  ),
  ttl: 1,
  proxied: false,
});
